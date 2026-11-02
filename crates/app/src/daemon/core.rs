//! O núcleo do daemon: reage a pedidos dos painéis, eventos da lâmpada e prazos, sem loop próprio
//! nem SO (o loop em `daemon::run` só entrega os eventos).

use std::time::{Duration, Instant};

use elgin_application::config::Settings;
use elgin_application::controller::{BulbController, DEFAULT_MIN_SEND_INTERVAL};
use elgin_application::ports::{
    BulbConnector, BulbDiscovery, BulbEvent, BulbObserver, Clock, CloudAccount, ConfigRepository,
    ControllerEvent,
};
use elgin_application::setup::{BulbSetup, RecoveryError, SetupState, recover_local_key};
use elgin_application::status::ConnectionStatus;
use elgin_application::supervisor::{
    ConnectionSupervisor, ConnectionTarget, RetryPolicy, SupervisorEvent,
};
use elgin_domain::{FavoriteColour, FavoriteSlot};
use elgin_ipc::messages::{PanelMessage, SetupRequestDto, SnapshotDto};
use tokio::sync::watch;

use crate::daemon::clock::TokioClock;
use crate::daemon::panel::{self, Anchor, PanelProcess};

/// Intervalo entre as consultas do login por QR durante a configuração inicial.
pub(crate) const SETUP_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Espera antes de tentar recuperar a chave de novo quando faltou internet.
pub(crate) const RECOVERY_RETRY: Duration = Duration::from_secs(300);

/// O controlador publica por aqui, mas o daemon recalcula o snapshot depois de cada evento.
#[derive(Debug, Default)]
pub(crate) struct NoopObserver;

impl BulbObserver for NoopObserver {
    fn notify(&mut self, _event: &ControllerEvent) {}
}

type Controller<K> = BulbController<<K as BulbConnector>::Gateway, NoopObserver, TokioClock>;

pub(crate) struct DaemonCore<R, K: BulbConnector, D, C, P> {
    settings: Settings<R>,
    connector: K,
    discovery: D,
    cloud: C,
    controller: Controller<K>,
    supervisor: Option<ConnectionSupervisor<K, D, TokioClock>>,
    setup: Option<BulbSetup<C, D, TokioClock>>,
    setup_poll_at: Option<Instant>,
    recovery_retry_at: Option<Instant>,
    panel: P,
    snapshots: watch::Sender<Option<SnapshotDto>>,
}

impl<R, K, D, C, P> DaemonCore<R, K, D, C, P>
where
    R: ConfigRepository,
    K: BulbConnector + Clone,
    D: BulbDiscovery + Clone,
    C: CloudAccount + Clone,
    P: PanelProcess,
{
    pub(crate) fn new(
        settings: Settings<R>,
        connector: K,
        discovery: D,
        cloud: C,
        panel: P,
        snapshots: watch::Sender<Option<SnapshotDto>>,
    ) -> Self {
        let mut core = Self {
            settings,
            connector,
            discovery,
            cloud,
            controller: BulbController::new(NoopObserver, TokioClock, DEFAULT_MIN_SEND_INTERVAL),
            supervisor: None,
            setup: None,
            setup_poll_at: None,
            recovery_retry_at: None,
            panel,
            snapshots,
        };
        core.connect_to_primary_bulb();
        core.publish();
        core
    }

    /// Quando o loop deve chamar [`on_deadline`](Self::on_deadline) de novo.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        [
            self.supervisor
                .as_ref()
                .and_then(ConnectionSupervisor::next_attempt_at),
            self.controller.next_flush_at(),
            self.setup_poll_at,
            self.recovery_retry_at,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Faz o que estiver vencido: tentar conectar, enviar comando pendente, consultar o login por
    /// QR, tentar recuperar a chave.
    pub(crate) async fn on_deadline(&mut self) {
        let now = TokioClock.now();
        if let Some(supervisor) = &mut self.supervisor {
            let events = supervisor.tick(&mut self.controller).await;
            self.on_supervisor_events(events).await;
        }
        // Falha no envio já foi tratada pelo controlador; a queda chega como evento da lâmpada.
        let _ = self.controller.flush().await;
        if self.setup_poll_at.is_some_and(|at| at <= now) {
            self.poll_setup().await;
        }
        if self.recovery_retry_at.is_some_and(|at| at <= now) {
            self.recovery_retry_at = None;
            self.recover_key().await;
        }
        self.publish();
    }

    /// Espera o próximo aviso da lâmpada conectada (para sempre, sem conexão).
    pub(crate) async fn next_bulb_event(&mut self) -> BulbEvent {
        match &mut self.supervisor {
            Some(supervisor) => supervisor.next_event().await,
            None => std::future::pending().await,
        }
    }

    pub(crate) async fn on_bulb_event(&mut self, event: BulbEvent) {
        match event {
            BulbEvent::StateReported(state) => self.controller.on_reported_state(state),
            BulbEvent::ConnectionLost(error) => {
                log::info!("conexão com a lâmpada perdida: {error}");
                if let Some(supervisor) = &mut self.supervisor {
                    let events = supervisor
                        .connection_lost(&mut self.controller, error)
                        .await;
                    self.on_supervisor_events(events).await;
                }
            }
        }
        self.publish();
    }

    pub(crate) async fn on_request(&mut self, message: PanelMessage) {
        match message {
            PanelMessage::Hello { .. } | PanelMessage::Close => {}
            PanelMessage::Command { command } => {
                let _ = self.controller.execute(command.to_command()).await;
            }
            PanelMessage::ApplyFavorite { slot } => {
                if let (Ok(slot), Some(state)) = (
                    FavoriteSlot::try_new(slot),
                    self.controller.snapshot().state,
                ) {
                    let command = self.settings.config().favorites.apply_command(slot, &state);
                    let _ = self.controller.execute(command).await;
                }
            }
            PanelMessage::SaveFavorite { slot } => {
                let state = self.controller.snapshot().state;
                if let (Ok(slot), Some(colour)) = (
                    FavoriteSlot::try_new(slot),
                    state.and_then(|state| FavoriteColour::of(&state.mode)),
                ) && let Err(error) = self.settings.save_favorite(slot, colour)
                {
                    log::warn!("não foi possível salvar a favorita: {error}");
                }
            }
            PanelMessage::Setup { request } => self.on_setup_request(request).await,
            PanelMessage::Activate => {
                if let Err(error) = panel::ensure_open(&mut self.panel, None) {
                    log::warn!("não foi possível abrir o painel: {error}");
                }
            }
        }
        self.publish();
    }

    /// Clique no ícone: abre ou fecha o painel.
    // Usado pela bandeja (#24).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn toggle_panel(&mut self, anchor: Option<Anchor>) {
        if let Err(error) = panel::toggle(&mut self.panel, anchor) {
            log::warn!("não foi possível abrir o painel: {error}");
        }
    }

    /// Antes de sair: fecha o painel.
    pub(crate) fn shutdown(&mut self) {
        self.panel.close();
    }

    /// Cria (ou aponta) o supervisor para a lâmpada principal da configuração.
    fn connect_to_primary_bulb(&mut self) {
        let Some(bulb) = self.settings.config().primary_bulb() else {
            self.controller
                .set_connection(ConnectionStatus::NotConfigured);
            return;
        };
        let target = ConnectionTarget::from(bulb);
        match &mut self.supervisor {
            Some(supervisor) => supervisor.update_target(target),
            None => {
                self.supervisor = Some(ConnectionSupervisor::new(
                    self.connector.clone(),
                    self.discovery.clone(),
                    TokioClock,
                    RetryPolicy::default(),
                    target,
                ));
                self.controller.set_connection(ConnectionStatus::Connecting);
            }
        }
    }

    async fn on_supervisor_events(&mut self, events: Vec<SupervisorEvent>) {
        for event in events {
            match event {
                SupervisorEvent::EndpointChanged(endpoint) => {
                    log::info!("lâmpada encontrada em {}", endpoint.ip);
                    let id = self
                        .settings
                        .config()
                        .primary_bulb()
                        .map(|bulb| bulb.id.clone());
                    if let Some(id) = id
                        && let Err(error) = self.settings.update_endpoint(&id, endpoint)
                    {
                        log::warn!("não foi possível salvar o novo endereço: {error}");
                    }
                }
                SupervisorEvent::KeyRejected => {
                    log::warn!("a lâmpada recusou a chave local; tentando recuperar pela nuvem");
                    self.recover_key().await;
                }
            }
        }
    }

    /// Uma tentativa de recuperar a chave pela nuvem (sem loop: nova tentativa só se faltou internet).
    async fn recover_key(&mut self) {
        let Some(id) = self
            .settings
            .config()
            .primary_bulb()
            .map(|bulb| bulb.id.clone())
        else {
            return;
        };
        match recover_local_key(&mut self.settings, &mut self.cloud, &id).await {
            Ok(bulb) => {
                log::info!("chave local recuperada pela nuvem");
                if let Some(supervisor) = &mut self.supervisor {
                    supervisor.update_target(ConnectionTarget::from(&bulb));
                }
            }
            Err(RecoveryError::NeedsReconfiguration(reason)) => {
                log::warn!("a chave não pôde ser recuperada ({reason}); é preciso reconfigurar");
                self.controller
                    .set_connection(ConnectionStatus::NeedsReconfiguration);
            }
            Err(RecoveryError::Temporary(reason)) => {
                log::info!("recuperação da chave adiada ({reason})");
                self.recovery_retry_at = Some(TokioClock.now() + RECOVERY_RETRY);
            }
        }
    }

    async fn on_setup_request(&mut self, request: SetupRequestDto) {
        match request {
            SetupRequestDto::Open => {
                self.setup.get_or_insert_with(|| {
                    BulbSetup::new(self.cloud.clone(), self.discovery.clone(), TokioClock)
                });
            }
            SetupRequestDto::SubmitUserCode { user_code } => {
                let setup = self.setup.get_or_insert_with(|| {
                    BulbSetup::new(self.cloud.clone(), self.discovery.clone(), TokioClock)
                });
                let state = setup.submit_user_code(&user_code).await;
                self.setup_poll_at = matches!(state, SetupState::WaitingConfirmation { .. })
                    .then(|| TokioClock.now() + SETUP_POLL_INTERVAL);
            }
            SetupRequestDto::Choose { device_id } => {
                let Some(setup) = &mut self.setup else { return };
                if matches!(
                    setup.choose(&device_id, &mut self.settings).await,
                    SetupState::Done { .. }
                ) {
                    log::info!("configuração inicial concluída");
                    self.connect_to_primary_bulb();
                }
            }
            SetupRequestDto::Cancel => {
                self.setup = None;
                self.setup_poll_at = None;
            }
        }
    }

    async fn poll_setup(&mut self) {
        self.setup_poll_at = None;
        let Some(setup) = &mut self.setup else { return };
        let state = setup.poll(&mut self.settings).await;
        if matches!(state, SetupState::WaitingConfirmation { .. }) {
            self.setup_poll_at = Some(TokioClock.now() + SETUP_POLL_INTERVAL);
        }
    }

    /// Manda o snapshot aos painéis, se mudou.
    fn publish(&mut self) {
        let config = self.settings.config();
        let snapshot = SnapshotDto::new(
            config.primary_bulb().map(|bulb| bulb.name.as_str()),
            &self.controller.snapshot(),
            &config.favorites,
            self.setup.as_ref().map(BulbSetup::state),
        );
        let connection = snapshot.connection;
        self.snapshots.send_if_modified(|current| {
            if current.as_ref().map(|previous| previous.connection) != Some(connection) {
                log::info!("conexão com a lâmpada: {connection:?}");
            }
            if current.as_ref() == Some(&snapshot) {
                false
            } else {
                *current = Some(snapshot);
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::panel::FakePanel;
    use elgin_application::config::{AppConfig, BulbConfig, CloudSession};
    use elgin_application::credentials::{Endpoint, LocalKey};
    use elgin_application::ports::{CloudDevice, CloudDevices, GatewayError, LoginPoll, QrLogin};
    use elgin_application::testing::{
        FakeBulbGateway, FakeConfigRepository, ScriptedEvents, StubCloudAccount, StubConnector,
        StubDiscovery,
    };
    use elgin_domain::{
        Brightness, BulbId, BulbName, BulbState, ColorTemperature, LightMode, Power,
    };
    use elgin_ipc::messages::{CommandDto, ConnectionDto, StateDto};
    use std::net::{IpAddr, Ipv4Addr};

    type Core =
        DaemonCore<FakeConfigRepository, StubConnector, StubDiscovery, StubCloudAccount, FakePanel>;

    fn white(percent: u8) -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::White {
                brightness: Brightness::from_percent(percent).unwrap(),
                temperature: ColorTemperature::WARMEST,
            },
        }
    }

    fn endpoint(last: u8) -> Endpoint {
        Endpoint {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, last)),
            protocol_version: "3.5".to_owned(),
        }
    }

    fn bulb(key: &str) -> BulbConfig {
        BulbConfig {
            id: BulbId::try_new("bf1").unwrap(),
            name: BulbName::try_new("Quarto").unwrap(),
            local_key: LocalKey::new(key),
            endpoint: Some(endpoint(10)),
        }
    }

    struct Fixture {
        core: Core,
        repository: FakeConfigRepository,
        connector: StubConnector,
        discovery: StubDiscovery,
        cloud: StubCloudAccount,
        snapshots: watch::Receiver<Option<SnapshotDto>>,
    }

    fn fixture(config: AppConfig) -> Fixture {
        let repository = FakeConfigRepository::with(config);
        let connector = StubConnector::new();
        let discovery = StubDiscovery::new();
        let cloud = StubCloudAccount::new();
        let (sender, snapshots) = watch::channel(None);
        let core = Core::new(
            Settings::load(repository.clone()).unwrap(),
            connector.clone(),
            discovery.clone(),
            cloud.clone(),
            FakePanel::default(),
            sender,
        );
        Fixture {
            core,
            repository,
            connector,
            discovery,
            cloud,
            snapshots,
        }
    }

    fn configured() -> Fixture {
        fixture(AppConfig {
            bulbs: vec![bulb("chave-velha")],
            cloud_session: Some(CloudSession::new("sessao")),
            ..AppConfig::default()
        })
    }

    impl Fixture {
        /// Roda tudo que está vencido agora (sem avançar o relógio).
        async fn settle(&mut self) {
            while let Some(deadline) = self.core.next_deadline() {
                if tokio::time::Instant::from_std(deadline) > tokio::time::Instant::now() {
                    break;
                }
                self.core.on_deadline().await;
            }
        }

        fn snapshot(&self) -> SnapshotDto {
            self.snapshots
                .borrow()
                .clone()
                .expect("nenhum snapshot publicado")
        }
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_bulb_the_snapshot_says_not_configured() {
        let f = fixture(AppConfig::default());

        assert_eq!(f.snapshot().connection, ConnectionDto::NotConfigured);
        assert_eq!(f.snapshot().bulb_name, None);
        assert_eq!(f.core.next_deadline(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn with_a_bulb_it_connects_and_publishes_the_state() {
        let mut f = configured();
        f.connector.succeed_with(FakeBulbGateway::new(white(40)));
        assert_eq!(f.snapshot().connection, ConnectionDto::Connecting);

        f.settle().await;

        let snapshot = f.snapshot();
        assert_eq!(snapshot.connection, ConnectionDto::Online);
        assert_eq!(snapshot.bulb_name.as_deref(), Some("Quarto"));
        assert_eq!(snapshot.state, Some(StateDto::from(&white(40))));
        assert_eq!(snapshot.favorites.len(), 6);
    }

    #[tokio::test(start_paused = true)]
    async fn panel_commands_reach_the_bulb() {
        let mut f = configured();
        let lamp = FakeBulbGateway::new(white(40));
        f.connector.succeed_with(lamp.clone());
        f.settle().await;

        f.core
            .on_request(PanelMessage::Command {
                command: CommandDto::SetBrightness { brightness: 800 },
            })
            .await;

        assert_eq!(lamp.applied(), [white(80)]);
        assert_eq!(f.snapshot().state, Some(StateDto::from(&white(80))));
    }

    #[tokio::test(start_paused = true)]
    async fn pushes_from_the_bulb_update_the_snapshot() {
        let mut f = configured();
        f.connector
            .succeed_with_events(FakeBulbGateway::new(white(40)), ScriptedEvents::new());
        f.settle().await;

        f.core
            .on_bulb_event(BulbEvent::StateReported(white(10)))
            .await;

        assert_eq!(f.snapshot().state, Some(StateDto::from(&white(10))));
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_connection_goes_offline_and_reconnects() {
        let mut f = configured();
        f.connector.succeed_with(FakeBulbGateway::new(white(40)));
        f.settle().await;

        f.core
            .on_bulb_event(BulbEvent::ConnectionLost(GatewayError::Disconnected))
            .await;
        assert_eq!(f.snapshot().connection, ConnectionDto::Offline);

        f.connector.succeed_with(FakeBulbGateway::new(white(40)));
        tokio::time::sleep(Duration::from_secs(1)).await;
        f.settle().await;
        assert_eq!(f.snapshot().connection, ConnectionDto::Online);
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejected_key_is_recovered_from_the_cloud_and_reconnects() {
        let mut f = configured();
        f.connector.fail_with(GatewayError::KeyRejected);
        f.cloud.list_returns(Ok(CloudDevices {
            devices: vec![CloudDevice {
                id: "bf1".to_owned(),
                name: "Quarto".to_owned(),
                local_key: LocalKey::new("chave-nova"),
                category: "dj".to_owned(),
                is_light: true,
                product_name: "10W".to_owned(),
            }],
            session: CloudSession::new("sessao-renovada"),
        }));
        f.connector.succeed_with(FakeBulbGateway::new(white(40)));

        f.settle().await;

        assert_eq!(f.connector.attempts()[1].key, LocalKey::new("chave-nova"));
        assert_eq!(f.snapshot().connection, ConnectionDto::Online);
        assert_eq!(
            f.repository.saved().unwrap().bulbs[0].local_key,
            LocalKey::new("chave-nova")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_rejected_key_that_cannot_be_recovered_needs_reconfiguration() {
        let mut f = configured();
        f.connector.fail_with(GatewayError::KeyRejected);
        f.cloud
            .list_returns(Err(elgin_application::ports::CloudError::SessionExpired));

        f.settle().await;

        assert_eq!(f.snapshot().connection, ConnectionDto::NeedsReconfiguration);
        assert_eq!(f.core.next_deadline(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn without_internet_the_recovery_is_retried_later() {
        let mut f = configured();
        f.connector.fail_with(GatewayError::KeyRejected);
        f.cloud
            .list_returns(Err(elgin_application::ports::CloudError::Network(
                "x".to_owned(),
            )));

        f.settle().await;

        assert_eq!(f.snapshot().connection, ConnectionDto::KeyRejected);
        let deadline = tokio::time::Instant::from_std(f.core.next_deadline().unwrap());
        assert_eq!(deadline - tokio::time::Instant::now(), RECOVERY_RETRY);
    }

    #[tokio::test(start_paused = true)]
    async fn favorites_are_applied_and_saved() {
        let mut f = configured();
        let lamp = FakeBulbGateway::new(white(40));
        f.connector.succeed_with(lamp.clone());
        f.settle().await;

        f.core
            .on_request(PanelMessage::ApplyFavorite { slot: 4 })
            .await;
        let LightMode::Colour(applied) = lamp.state().mode else {
            panic!("deveria estar no modo cor")
        };
        assert_eq!(applied.hue().degrees(), 240);

        tokio::time::sleep(DEFAULT_MIN_SEND_INTERVAL).await;
        f.core
            .on_request(PanelMessage::Command {
                command: CommandDto::SetColour {
                    hue: 170,
                    saturation: 600,
                    value: 400,
                },
            })
            .await;
        f.core
            .on_request(PanelMessage::SaveFavorite { slot: 0 })
            .await;

        let saved = f.repository.saved().unwrap().favorites;
        let first = saved.get(FavoriteSlot::try_new(0).unwrap());
        assert_eq!(first.hue().degrees(), 170);
        assert_eq!(first.saturation().permille(), 600);
    }

    #[tokio::test(start_paused = true)]
    async fn the_initial_setup_ends_connected_to_the_chosen_bulb() {
        let mut f = fixture(AppConfig::default());
        f.cloud.start_returns(Ok(QrLogin {
            user_code: "Abc1234".to_owned(),
            token: "t".to_owned(),
            qr_content: "tuyaSmart--qrLogin?token=t".to_owned(),
        }));
        f.cloud
            .poll_returns(Ok(LoginPoll::Done(CloudSession::new("sessao"))));
        f.cloud.list_returns(Ok(CloudDevices {
            devices: vec![CloudDevice {
                id: "bf9".to_owned(),
                name: "Sala".to_owned(),
                local_key: LocalKey::new("chave-sala-00000"),
                category: "dj".to_owned(),
                is_light: true,
                product_name: "10W".to_owned(),
            }],
            session: CloudSession::new("sessao"),
        }));

        f.core
            .on_request(PanelMessage::Setup {
                request: SetupRequestDto::Open,
            })
            .await;
        f.core
            .on_request(PanelMessage::Setup {
                request: SetupRequestDto::SubmitUserCode {
                    user_code: "Abc1234".to_owned(),
                },
            })
            .await;
        assert!(matches!(
            f.snapshot().setup,
            Some(elgin_ipc::messages::SetupStateDto::WaitingConfirmation { .. })
        ));

        tokio::time::sleep(SETUP_POLL_INTERVAL).await;
        f.settle().await;
        assert!(matches!(
            f.snapshot().setup,
            Some(elgin_ipc::messages::SetupStateDto::ChooseDevice { .. })
        ));

        f.discovery.answer(Some(endpoint(33)));
        f.connector.succeed_with(FakeBulbGateway::new(white(70)));
        f.core
            .on_request(PanelMessage::Setup {
                request: SetupRequestDto::Choose {
                    device_id: "bf9".to_owned(),
                },
            })
            .await;
        f.settle().await;

        assert_eq!(f.snapshot().bulb_name.as_deref(), Some("Sala"));
        assert_eq!(f.snapshot().connection, ConnectionDto::Online);
        assert_eq!(
            f.connector.attempts()[0].key,
            LocalKey::new("chave-sala-00000")
        );
        assert_eq!(f.connector.attempts()[0].endpoint, endpoint(33));
    }

    #[tokio::test(start_paused = true)]
    async fn activate_opens_the_panel_and_toggle_switches_it() {
        let mut f = fixture(AppConfig::default());

        f.core.on_request(PanelMessage::Activate).await;
        assert!(f.core.panel.open);

        f.core.toggle_panel(None);
        assert!(!f.core.panel.open);

        f.core.shutdown();
        assert!(!f.core.panel.open);
    }
}
