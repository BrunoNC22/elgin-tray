//! Configuração inicial da lâmpada pelo login com QR code, e recuperação da local key.

use std::time::{Duration, Instant};

use elgin_domain::{BulbId, BulbName};

use crate::config::{BulbConfig, CloudSession, Settings};
use crate::ports::{
    BulbDiscovery, Clock, CloudAccount, CloudDevice, CloudError, ConfigRepository, LoginPoll,
    QrLogin,
};

/// Quanto esperar a confirmação do QR no celular.
pub const QR_TIMEOUT: Duration = Duration::from_secs(150);
/// Quanto esperar a lâmpada responder à descoberta depois de escolhida.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);

/// Uma lâmpada da conta, para o usuário escolher (sem a chave).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDevice {
    pub id: String,
    pub name: String,
    pub product_name: String,
}

/// Em que passo está a configuração (o painel desenha cada um).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupState {
    /// Esperando o código de usuário do Smart Life.
    AwaitingUserCode,
    /// QR code na tela, esperando a confirmação no celular.
    WaitingConfirmation { qr_content: String },
    /// Login feito: o usuário escolhe a lâmpada.
    ChooseDevice { devices: Vec<SetupDevice> },
    /// Procurando a lâmpada escolhida na rede.
    Discovering,
    /// Pronto. `found_on_network` diz se o IP já foi achado (senão o app continua procurando).
    Done {
        bulb: BulbName,
        found_on_network: bool,
    },
    /// Deu errado; o usuário pode mandar o código de novo.
    Failed { reason: String },
}

/// O fluxo de configuração inicial, passo a passo.
///
/// Sem loop próprio: o painel chama [`submit_user_code`](Self::submit_user_code) e
/// [`choose`](Self::choose); o daemon chama [`poll`](Self::poll) a cada poucos segundos enquanto o
/// estado é `WaitingConfirmation`.
#[derive(Debug)]
pub struct BulbSetup<C, D, K> {
    cloud: C,
    discovery: D,
    clock: K,
    state: SetupState,
    login: Option<QrLogin>,
    started_at: Option<Instant>,
    session: Option<CloudSession>,
    devices: Vec<CloudDevice>,
}

impl<C, D, K> BulbSetup<C, D, K>
where
    C: CloudAccount,
    D: BulbDiscovery,
    K: Clock,
{
    pub fn new(cloud: C, discovery: D, clock: K) -> Self {
        Self {
            cloud,
            discovery,
            clock,
            state: SetupState::AwaitingUserCode,
            login: None,
            started_at: None,
            session: None,
            devices: Vec::new(),
        }
    }

    pub fn state(&self) -> &SetupState {
        &self.state
    }

    /// Começa o login com o código de usuário (vale também para tentar de novo depois de um erro).
    pub async fn submit_user_code(&mut self, user_code: &str) -> &SetupState {
        self.cancel();
        match self.cloud.start_qr_login(user_code.trim()).await {
            Ok(login) => {
                self.state = SetupState::WaitingConfirmation {
                    qr_content: login.qr_content.clone(),
                };
                self.login = Some(login);
                self.started_at = Some(self.clock.now());
            }
            Err(error) => self.fail(error.to_string()),
        }
        &self.state
    }

    /// Consulta o login. Confirmado: salva a sessão e lista as lâmpadas da conta.
    pub async fn poll<R: ConfigRepository>(&mut self, settings: &mut Settings<R>) -> &SetupState {
        let (Some(login), Some(started_at)) = (self.login.clone(), self.started_at) else {
            return &self.state;
        };
        if self.clock.now() >= started_at + QR_TIMEOUT {
            self.fail("o QR code expirou sem confirmação; mande o código de novo".to_owned());
            return &self.state;
        }
        match self.cloud.poll_login(&login).await {
            Ok(LoginPoll::Pending) => {}
            Ok(LoginPoll::Done(session)) => self.list_devices(session, settings).await,
            Err(error) => self.fail(error.to_string()),
        }
        &self.state
    }

    async fn list_devices<R: ConfigRepository>(
        &mut self,
        session: CloudSession,
        settings: &mut Settings<R>,
    ) {
        self.login = None;
        let listed = match self.cloud.list_devices(&session).await {
            Ok(listed) => listed,
            Err(error) => return self.fail(error.to_string()),
        };
        // A sessão é guardada já: mesmo que o usuário desista agora, ela serve para recuperar a chave.
        if let Err(error) = settings.set_cloud_session(Some(listed.session.clone())) {
            return self.fail(error.to_string());
        }
        self.session = Some(listed.session);

        let lights: Vec<CloudDevice> = listed
            .devices
            .iter()
            .filter(|device| device.is_light)
            .cloned()
            .collect();
        self.devices = if lights.is_empty() {
            listed.devices
        } else {
            lights
        };
        if self.devices.is_empty() {
            return self.fail("nenhum dispositivo nesta conta do Smart Life".to_owned());
        }
        self.state = SetupState::ChooseDevice {
            devices: self
                .devices
                .iter()
                .map(|device| SetupDevice {
                    id: device.id.clone(),
                    name: device.name.clone(),
                    product_name: device.product_name.clone(),
                })
                .collect(),
        };
    }

    /// O usuário escolheu a lâmpada `device_id`: procura na rede e salva a configuração.
    /// Um id que não está na lista não muda nada.
    pub async fn choose<R: ConfigRepository>(
        &mut self,
        device_id: &str,
        settings: &mut Settings<R>,
    ) -> &SetupState {
        if !matches!(self.state, SetupState::ChooseDevice { .. }) {
            return &self.state;
        }
        let Some(device) = self
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .cloned()
        else {
            return &self.state;
        };
        let id = match BulbId::try_new(device.id) {
            Ok(id) => id,
            Err(error) => {
                self.fail(error.to_string());
                return &self.state;
            }
        };
        let name = BulbName::try_new(device.name)
            .unwrap_or_else(|_| BulbName::try_new("Lâmpada").expect("nome padrão válido"));

        self.state = SetupState::Discovering;
        // Não achar na rede agora não impede a configuração: o supervisor continua procurando.
        let endpoint = self.discovery.find(&id, DISCOVERY_TIMEOUT).await;
        let found_on_network = endpoint.is_some();
        let bulb = BulbConfig {
            id,
            name: name.clone(),
            local_key: device.local_key,
            endpoint,
        };
        self.state = match settings.set_primary_bulb(bulb) {
            Ok(()) => SetupState::Done {
                bulb: name,
                found_on_network,
            },
            Err(error) => SetupState::Failed {
                reason: error.to_string(),
            },
        };
        &self.state
    }

    /// Volta ao começo.
    pub fn cancel(&mut self) {
        self.state = SetupState::AwaitingUserCode;
        self.login = None;
        self.started_at = None;
        self.devices.clear();
    }

    fn fail(&mut self, reason: String) {
        self.login = None;
        self.started_at = None;
        self.state = SetupState::Failed { reason };
    }
}

/// Por que a local key não pôde ser recuperada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryError {
    /// Problema passageiro (sem internet): tentar de novo mais tarde.
    Temporary(String),
    /// Não dá para resolver sozinho: o usuário precisa refazer a configuração.
    NeedsReconfiguration(String),
}

/// A lâmpada recusou a local key: busca a chave atual na nuvem com a sessão salva e grava.
///
/// Devolve a configuração da lâmpada com a chave nova. Se a nuvem devolver a **mesma** chave, a
/// recuperação não resolve (a lâmpada pode ter sido resetada sem parear de novo): pede reconfiguração.
pub async fn recover_local_key<R, C>(
    settings: &mut Settings<R>,
    cloud: &mut C,
    bulb_id: &BulbId,
) -> Result<BulbConfig, RecoveryError>
where
    R: ConfigRepository,
    C: CloudAccount,
{
    let needs_setup = |reason: &str| RecoveryError::NeedsReconfiguration(reason.to_owned());
    let current = settings
        .config()
        .bulbs
        .iter()
        .find(|bulb| &bulb.id == bulb_id)
        .cloned()
        .ok_or_else(|| needs_setup("a lâmpada não está na configuração"))?;
    let session = settings
        .config()
        .cloud_session
        .clone()
        .ok_or_else(|| needs_setup("não há sessão da nuvem salva"))?;

    let listed = match cloud.list_devices(&session).await {
        Ok(listed) => listed,
        Err(CloudError::Network(detail)) => return Err(RecoveryError::Temporary(detail)),
        Err(error) => return Err(RecoveryError::NeedsReconfiguration(error.to_string())),
    };
    settings
        .set_cloud_session(Some(listed.session))
        .map_err(|error| RecoveryError::Temporary(error.to_string()))?;

    let device = listed
        .devices
        .into_iter()
        .find(|device| device.id == bulb_id.as_str())
        .ok_or_else(|| needs_setup("a lâmpada não está mais na conta do Smart Life"))?;
    if device.local_key == current.local_key {
        return Err(needs_setup(
            "a nuvem informa a mesma chave que a lâmpada recusou",
        ));
    }

    let recovered = BulbConfig {
        local_key: device.local_key,
        ..current
    };
    settings
        .set_primary_bulb(recovered.clone())
        .map_err(|error| RecoveryError::Temporary(error.to_string()))?;
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::credentials::{Endpoint, LocalKey};
    use crate::ports::CloudDevices;
    use crate::testing::{FakeConfigRepository, ManualClock, StubCloudAccount, StubDiscovery};
    use std::net::{IpAddr, Ipv4Addr};

    type Setup = BulbSetup<StubCloudAccount, StubDiscovery, ManualClock>;

    fn qr_login() -> QrLogin {
        QrLogin {
            user_code: "Abc1234".to_owned(),
            token: "token".to_owned(),
            qr_content: "tuyaSmart--qrLogin?token=token".to_owned(),
        }
    }

    fn cloud_device(id: &str, name: &str, is_light: bool) -> CloudDevice {
        CloudDevice {
            id: id.to_owned(),
            name: name.to_owned(),
            local_key: LocalKey::new(format!("chave-{id}")),
            category: if is_light { "dj" } else { "cz" }.to_owned(),
            is_light,
            product_name: "Produto".to_owned(),
        }
    }

    fn setup_device(id: &str, name: &str) -> SetupDevice {
        SetupDevice {
            id: id.to_owned(),
            name: name.to_owned(),
            product_name: "Produto".to_owned(),
        }
    }

    fn endpoint() -> Endpoint {
        Endpoint {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, 50)),
            protocol_version: "3.5".to_owned(),
        }
    }

    struct Fixture {
        setup: Setup,
        cloud: StubCloudAccount,
        discovery: StubDiscovery,
        clock: ManualClock,
        settings: Settings<FakeConfigRepository>,
        repository: FakeConfigRepository,
    }

    fn fixture() -> Fixture {
        let cloud = StubCloudAccount::new();
        let discovery = StubDiscovery::new();
        let clock = ManualClock::new();
        let repository = FakeConfigRepository::empty();
        Fixture {
            setup: Setup::new(cloud.clone(), discovery.clone(), clock.clone()),
            settings: Settings::load(repository.clone()).unwrap(),
            cloud,
            discovery,
            clock,
            repository,
        }
    }

    /// Fixture já com o login confirmado e a conta com estes dispositivos.
    async fn logged_in(devices: Vec<CloudDevice>) -> Fixture {
        let mut f = fixture();
        f.cloud.start_returns(Ok(qr_login()));
        f.cloud
            .poll_returns(Ok(LoginPoll::Done(CloudSession::new("sessao"))));
        f.cloud.list_returns(Ok(CloudDevices {
            devices,
            session: CloudSession::new("sessao-renovada"),
        }));
        f.setup.submit_user_code("Abc1234").await;
        f.setup.poll(&mut f.settings).await;
        f
    }

    #[test]
    fn starts_waiting_for_the_user_code() {
        assert_eq!(fixture().setup.state(), &SetupState::AwaitingUserCode);
    }

    #[tokio::test]
    async fn the_user_code_shows_the_qr() {
        let mut f = fixture();
        f.cloud.start_returns(Ok(qr_login()));

        let state = f.setup.submit_user_code("Abc1234").await.clone();

        assert_eq!(
            state,
            SetupState::WaitingConfirmation {
                qr_content: "tuyaSmart--qrLogin?token=token".to_owned()
            }
        );
        assert_eq!(f.cloud.user_codes(), ["Abc1234"]);
    }

    #[tokio::test]
    async fn an_invalid_user_code_fails_and_can_be_retried() {
        let mut f = fixture();
        f.cloud
            .start_returns(Err(CloudError::Rejected("user code invalid".to_owned())));
        f.cloud.start_returns(Ok(qr_login()));

        let failed = f.setup.submit_user_code("errado1").await.clone();
        let retried = f.setup.submit_user_code("Abc1234").await.clone();

        assert!(
            matches!(failed, SetupState::Failed { reason } if reason.contains("user code invalid"))
        );
        assert!(matches!(retried, SetupState::WaitingConfirmation { .. }));
    }

    #[tokio::test]
    async fn polling_while_pending_keeps_waiting() {
        let mut f = fixture();
        f.cloud.start_returns(Ok(qr_login()));
        f.setup.submit_user_code("Abc1234").await;

        let state = f.setup.poll(&mut f.settings).await.clone();

        assert!(matches!(state, SetupState::WaitingConfirmation { .. }));
    }

    #[tokio::test]
    async fn a_qr_not_confirmed_in_time_fails() {
        let mut f = fixture();
        f.cloud.start_returns(Ok(qr_login()));
        f.setup.submit_user_code("Abc1234").await;

        f.clock.advance(QR_TIMEOUT);
        let state = f.setup.poll(&mut f.settings).await.clone();

        assert!(matches!(state, SetupState::Failed { reason } if reason.contains("expirou")));
    }

    #[tokio::test]
    async fn polling_before_submitting_does_nothing() {
        let mut f = fixture();

        assert_eq!(
            f.setup.poll(&mut f.settings).await,
            &SetupState::AwaitingUserCode
        );
        assert_eq!(f.cloud.poll_count(), 0);
    }

    #[tokio::test]
    async fn a_confirmed_login_saves_the_session_and_lists_only_the_lights() {
        let f = logged_in(vec![
            cloud_device("bf1", "Quarto", true),
            cloud_device("cz1", "Tomada", false),
            cloud_device("bf2", "Sala", true),
        ])
        .await;

        assert_eq!(
            f.setup.state(),
            &SetupState::ChooseDevice {
                devices: vec![setup_device("bf1", "Quarto"), setup_device("bf2", "Sala")]
            }
        );
        assert_eq!(f.cloud.listed_sessions(), [CloudSession::new("sessao")]);
        assert_eq!(
            f.repository.saved().unwrap().cloud_session,
            Some(CloudSession::new("sessao-renovada"))
        );
    }

    #[tokio::test]
    async fn without_lights_every_device_is_listed() {
        let f = logged_in(vec![cloud_device("cz1", "Tomada", false)]).await;

        assert_eq!(
            f.setup.state(),
            &SetupState::ChooseDevice {
                devices: vec![setup_device("cz1", "Tomada")]
            }
        );
    }

    #[tokio::test]
    async fn an_account_without_devices_fails() {
        let f = logged_in(Vec::new()).await;

        assert!(
            matches!(f.setup.state(), SetupState::Failed { reason } if reason.contains("nenhum dispositivo"))
        );
    }

    #[tokio::test]
    async fn choosing_a_bulb_found_on_the_network_saves_it_with_the_address() {
        let mut f = logged_in(vec![cloud_device("bf1", "Quarto", true)]).await;
        f.discovery.answer(Some(endpoint()));

        let state = f.setup.choose("bf1", &mut f.settings).await.clone();

        assert_eq!(
            state,
            SetupState::Done {
                bulb: BulbName::try_new("Quarto").unwrap(),
                found_on_network: true
            }
        );
        let saved = f.repository.saved().unwrap();
        assert_eq!(
            saved.bulbs,
            [BulbConfig {
                id: BulbId::try_new("bf1").unwrap(),
                name: BulbName::try_new("Quarto").unwrap(),
                local_key: LocalKey::new("chave-bf1"),
                endpoint: Some(endpoint()),
            }]
        );
        assert_eq!(f.discovery.searches()[0].1, DISCOVERY_TIMEOUT);
    }

    #[tokio::test]
    async fn a_bulb_not_found_on_the_network_is_saved_anyway() {
        let mut f = logged_in(vec![cloud_device("bf1", "Quarto", true)]).await;

        let state = f.setup.choose("bf1", &mut f.settings).await.clone();

        assert_eq!(
            state,
            SetupState::Done {
                bulb: BulbName::try_new("Quarto").unwrap(),
                found_on_network: false
            }
        );
        assert_eq!(f.repository.saved().unwrap().bulbs[0].endpoint, None);
    }

    #[tokio::test]
    async fn a_device_without_a_name_gets_a_default_one() {
        let mut f = logged_in(vec![cloud_device("bf1", "  ", true)]).await;

        f.setup.choose("bf1", &mut f.settings).await;

        assert_eq!(
            f.repository.saved().unwrap().bulbs[0].name.as_str(),
            "Lâmpada"
        );
    }

    #[tokio::test]
    async fn choosing_an_unknown_device_changes_nothing() {
        let mut f = logged_in(vec![cloud_device("bf1", "Quarto", true)]).await;

        let state = f.setup.choose("outro", &mut f.settings).await.clone();

        assert!(matches!(state, SetupState::ChooseDevice { .. }));
        assert!(f.repository.saved().unwrap().bulbs.is_empty());
    }

    #[tokio::test]
    async fn a_listing_error_after_login_fails() {
        let mut f = fixture();
        f.cloud.start_returns(Ok(qr_login()));
        f.cloud
            .poll_returns(Ok(LoginPoll::Done(CloudSession::new("sessao"))));
        f.cloud
            .list_returns(Err(CloudError::Network("sem internet".to_owned())));
        f.setup.submit_user_code("Abc1234").await;

        let state = f.setup.poll(&mut f.settings).await.clone();

        assert!(matches!(state, SetupState::Failed { reason } if reason.contains("sem internet")));
    }

    #[tokio::test]
    async fn cancel_goes_back_to_the_start() {
        let mut f = logged_in(vec![cloud_device("bf1", "Quarto", true)]).await;

        f.setup.cancel();

        assert_eq!(f.setup.state(), &SetupState::AwaitingUserCode);
    }

    // --- Recuperação da local key ---

    fn configured(
        key: &str,
        session: Option<&str>,
    ) -> (Settings<FakeConfigRepository>, FakeConfigRepository) {
        let repository = FakeConfigRepository::with(AppConfig {
            bulbs: vec![BulbConfig {
                id: BulbId::try_new("bf1").unwrap(),
                name: BulbName::try_new("Quarto").unwrap(),
                local_key: LocalKey::new(key),
                endpoint: Some(endpoint()),
            }],
            cloud_session: session.map(CloudSession::new),
            ..AppConfig::default()
        });
        (Settings::load(repository.clone()).unwrap(), repository)
    }

    fn bulb_id() -> BulbId {
        BulbId::try_new("bf1").unwrap()
    }

    #[tokio::test]
    async fn recovery_fetches_and_saves_the_new_key() {
        let (mut settings, repository) = configured("chave-velha", Some("sessao"));
        let mut cloud = StubCloudAccount::new();
        let mut renewed = cloud_device("bf1", "Quarto", true);
        renewed.local_key = LocalKey::new("chave-nova");
        cloud.list_returns(Ok(CloudDevices {
            devices: vec![renewed],
            session: CloudSession::new("sessao-renovada"),
        }));

        let recovered = recover_local_key(&mut settings, &mut cloud, &bulb_id())
            .await
            .unwrap();

        assert_eq!(recovered.local_key, LocalKey::new("chave-nova"));
        assert_eq!(recovered.endpoint, Some(endpoint()), "mantém o endereço");
        let saved = repository.saved().unwrap();
        assert_eq!(saved.bulbs[0].local_key, LocalKey::new("chave-nova"));
        assert_eq!(
            saved.cloud_session,
            Some(CloudSession::new("sessao-renovada"))
        );
    }

    #[tokio::test]
    async fn recovery_without_a_session_needs_reconfiguration() {
        let (mut settings, _) = configured("chave-velha", None);

        let result =
            recover_local_key(&mut settings, &mut StubCloudAccount::new(), &bulb_id()).await;

        assert!(matches!(
            result,
            Err(RecoveryError::NeedsReconfiguration(_))
        ));
    }

    #[tokio::test]
    async fn recovery_with_an_expired_session_needs_reconfiguration() {
        let (mut settings, _) = configured("chave-velha", Some("sessao"));
        let mut cloud = StubCloudAccount::new();
        cloud.list_returns(Err(CloudError::SessionExpired));

        let result = recover_local_key(&mut settings, &mut cloud, &bulb_id()).await;

        assert!(matches!(
            result,
            Err(RecoveryError::NeedsReconfiguration(_))
        ));
    }

    #[tokio::test]
    async fn recovery_without_internet_is_temporary() {
        let (mut settings, _) = configured("chave-velha", Some("sessao"));
        let mut cloud = StubCloudAccount::new();
        cloud.list_returns(Err(CloudError::Network("sem internet".to_owned())));

        let result = recover_local_key(&mut settings, &mut cloud, &bulb_id()).await;

        assert!(matches!(result, Err(RecoveryError::Temporary(_))));
    }

    #[tokio::test]
    async fn recovery_needs_reconfiguration_when_the_bulb_left_the_account_or_the_key_is_the_same()
    {
        for devices in [Vec::new(), vec![cloud_device("bf1", "Quarto", true)]] {
            let (mut settings, _) = configured("chave-bf1", Some("sessao"));
            let mut cloud = StubCloudAccount::new();
            cloud.list_returns(Ok(CloudDevices {
                devices,
                session: CloudSession::new("sessao"),
            }));

            let result = recover_local_key(&mut settings, &mut cloud, &bulb_id()).await;

            assert!(
                matches!(result, Err(RecoveryError::NeedsReconfiguration(_))),
                "{result:?}"
            );
        }
    }
}
