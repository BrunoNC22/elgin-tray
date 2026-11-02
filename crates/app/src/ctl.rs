//! `elgin-tray ctl`: controla a lâmpada pelo terminal (adaptador driving de desenvolvimento).
//!
//! Lê a lâmpada principal da configuração, acha o endereço (salvo ou pela rede), conecta e executa
//! o comando. Se a lâmpada não responde no endereço salvo, procura de novo na rede e salva o novo.

use std::time::Duration;

use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use tokio::time::Instant;

use elgin_application::config::{BulbConfig, Settings};
use elgin_application::credentials::Endpoint;
use elgin_application::ports::{
    BulbConnector, BulbDiscovery, BulbEvent, BulbEvents, BulbGateway, CloudAccount,
    ConfigRepository, GatewayError, LoginPoll,
};
use elgin_domain::{BulbState, LightMode, Power};

use crate::args::CtlCommand;

/// Quanto esperar a lâmpada responder ao pedido de descoberta.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(6);
/// Intervalo entre as consultas do login por QR.
const LOGIN_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Quanto esperar a confirmação do QR no celular (o QR expira em poucos minutos).
const LOGIN_TIMEOUT: Duration = Duration::from_secs(150);

/// Para onde vão as mensagens (o terminal, no app; um gravador, nos testes).
pub(crate) trait Output {
    fn line(&mut self, text: &str);
}

/// Saída padrão do processo.
#[derive(Debug, Default)]
pub(crate) struct Stdout;

impl Output for Stdout {
    fn line(&mut self, text: &str) {
        println!("{text}");
    }
}

/// O estado em uma linha, em português.
pub(crate) fn describe_state(state: &BulbState) -> String {
    let power = match state.power {
        Power::On => "ligada",
        Power::Off => "desligada",
    };
    let mode = match state.mode {
        LightMode::White {
            brightness,
            temperature,
        } => {
            let feel = match temperature.percent() {
                0..=33 => "quente",
                34..=66 => "neutra",
                _ => "fria",
            };
            format!(
                "branco, brilho {}%, temperatura {}% ({feel})",
                brightness.percent(),
                temperature.percent()
            )
        }
        LightMode::Colour(hsv) => format!(
            "cor (matiz {}°, saturação {}%), brilho {}%",
            hsv.hue().degrees(),
            hsv.saturation().percent(),
            hsv.value().percent()
        ),
        LightMode::Scene => "cena".to_owned(),
        LightMode::Music => "música".to_owned(),
    };
    format!("{power}, {mode}")
}

/// Executa um comando do ctl. O erro é a mensagem para o usuário.
pub(crate) async fn run<R, K, D, C, O>(
    command: CtlCommand,
    settings: &mut Settings<R>,
    connector: &mut K,
    discovery: &mut D,
    cloud: &mut C,
    out: &mut O,
) -> Result<(), String>
where
    R: ConfigRepository,
    K: BulbConnector,
    D: BulbDiscovery,
    C: CloudAccount,
    O: Output,
{
    // Os comandos da nuvem não precisam de lâmpada configurada.
    match &command {
        CtlCommand::Login(user_code) => return login(user_code, settings, cloud, out).await,
        CtlCommand::Devices => return list_cloud_devices(settings, cloud, out).await,
        _ => {}
    }
    let bulb = settings.config().primary_bulb().cloned().ok_or_else(|| {
        "nenhuma lâmpada configurada: preencha o config.toml (veja o README)".to_owned()
    })?;
    let show = |out: &mut O, state: &BulbState| {
        out.line(&format!(
            "{}: {}",
            bulb.name.as_str(),
            describe_state(state)
        ));
    };

    match command {
        CtlCommand::Login(_) | CtlCommand::Devices => unreachable!("tratado acima"),
        CtlCommand::Discover => {
            let endpoint = find_and_save(&bulb, settings, discovery, out).await?;
            out.line(&format!(
                "{}: {} (protocolo {})",
                bulb.name.as_str(),
                endpoint.ip,
                endpoint.protocol_version
            ));
        }
        CtlCommand::Status => {
            let (mut gateway, _events) =
                connect(&bulb, settings, connector, discovery, out).await?;
            let state = gateway.fetch_state().await.map_err(describe_error)?;
            show(out, &state);
        }
        CtlCommand::Apply(command) => {
            let (mut gateway, _events) =
                connect(&bulb, settings, connector, discovery, out).await?;
            let state = gateway.fetch_state().await.map_err(describe_error)?;
            let target = state.apply(&command);
            gateway.apply(&target).await.map_err(describe_error)?;
            show(out, &target);
        }
        CtlCommand::Watch => {
            let (mut gateway, mut events) =
                connect(&bulb, settings, connector, discovery, out).await?;
            let state = gateway.fetch_state().await.map_err(describe_error)?;
            show(out, &state);
            loop {
                match events.next_event().await {
                    BulbEvent::StateReported(state) => show(out, &state),
                    BulbEvent::ConnectionLost(error) => return Err(describe_error(error)),
                }
            }
        }
    }
    Ok(())
}

/// Login por QR code: mostra o QR, espera a confirmação no celular e guarda a sessão no config.
async fn login<R, C, O>(
    user_code: &str,
    settings: &mut Settings<R>,
    cloud: &mut C,
    out: &mut O,
) -> Result<(), String>
where
    R: ConfigRepository,
    C: CloudAccount,
    O: Output,
{
    let login = cloud
        .start_qr_login(user_code)
        .await
        .map_err(|error| error.to_string())?;
    out.line("Leia este QR code no app Smart Life (+ → Escanear) e confirme o login:");
    out.line(&render_qr(&login.qr_content));
    out.line(&format!("(conteúdo do QR code: {})", login.qr_content));

    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        match cloud
            .poll_login(&login)
            .await
            .map_err(|error| error.to_string())?
        {
            LoginPoll::Done(session) => {
                settings
                    .set_cloud_session(Some(session))
                    .map_err(|error| error.to_string())?;
                out.line("Login confirmado. A sessão da nuvem foi salva no config.toml.");
                return Ok(());
            }
            LoginPoll::Pending if Instant::now() >= deadline => {
                return Err(
                    "o QR code não foi confirmado a tempo; rode o comando de novo".to_owned(),
                );
            }
            LoginPoll::Pending => tokio::time::sleep(LOGIN_POLL_INTERVAL).await,
        }
    }
}

/// Lista os dispositivos da conta na nuvem e guarda a sessão (que pode ter sido renovada).
async fn list_cloud_devices<R, C, O>(
    settings: &mut Settings<R>,
    cloud: &mut C,
    out: &mut O,
) -> Result<(), String>
where
    R: ConfigRepository,
    C: CloudAccount,
    O: Output,
{
    let session = settings.config().cloud_session.clone().ok_or_else(|| {
        "não há sessão da nuvem: rode `elgin-tray ctl login <código de usuário>` antes".to_owned()
    })?;
    let listed = cloud
        .list_devices(&session)
        .await
        .map_err(|error| error.to_string())?;
    settings
        .set_cloud_session(Some(listed.session))
        .map_err(|error| error.to_string())?;
    for device in listed.devices {
        // Só o começo da chave: o terminal pode estar sendo compartilhado ou gravado.
        let key_start: String = device.local_key.expose().chars().take(4).collect();
        out.line(&format!(
            "{} ({}, {}): id {}, chave {key_start}…",
            device.name, device.category, device.product_name, device.id
        ));
    }
    Ok(())
}

/// QR code em caracteres de bloco, claro sobre escuro (lê melhor em terminais de fundo escuro).
fn render_qr(content: &str) -> String {
    match QrCode::new(content) {
        Ok(code) => code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build(),
        Err(_) => String::new(),
    }
}

/// Conecta na lâmpada. Sem endereço salvo, procura na rede; se o endereço salvo não responde,
/// procura de novo (o IP pode ter mudado) e tenta o novo.
async fn connect<R, K, D, O>(
    bulb: &BulbConfig,
    settings: &mut Settings<R>,
    connector: &mut K,
    discovery: &mut D,
    out: &mut O,
) -> Result<(K::Gateway, K::Events), String>
where
    R: ConfigRepository,
    K: BulbConnector,
    D: BulbDiscovery,
    O: Output,
{
    let (endpoint, saved) = match bulb.endpoint.clone() {
        Some(endpoint) => (endpoint, true),
        None => (find_and_save(bulb, settings, discovery, out).await?, false),
    };
    match connector
        .connect(&bulb.id, &bulb.local_key, &endpoint)
        .await
    {
        Ok(connection) => Ok(connection),
        Err(GatewayError::Unreachable | GatewayError::Timeout) if saved => {
            out.line(&format!(
                "A lâmpada não respondeu em {}; procurando na rede...",
                endpoint.ip
            ));
            let found = find_and_save(bulb, settings, discovery, out).await?;
            if found == endpoint {
                return Err(describe_error(GatewayError::Unreachable));
            }
            connector
                .connect(&bulb.id, &bulb.local_key, &found)
                .await
                .map_err(describe_error)
        }
        Err(error) => Err(describe_error(error)),
    }
}

async fn find_and_save<R, D, O>(
    bulb: &BulbConfig,
    settings: &mut Settings<R>,
    discovery: &mut D,
    out: &mut O,
) -> Result<Endpoint, String>
where
    R: ConfigRepository,
    D: BulbDiscovery,
    O: Output,
{
    let endpoint = discovery
        .find(&bulb.id, DISCOVERY_TIMEOUT)
        .await
        .ok_or_else(|| {
            format!(
                "a lâmpada \"{}\" não foi encontrada na rede (ela está ligada no interruptor e \
                 no Wi-Fi 2,4 GHz da mesma rede do PC?)",
                bulb.name.as_str()
            )
        })?;
    if let Err(error) = settings.update_endpoint(&bulb.id, endpoint.clone()) {
        out.line(&format!("Aviso: o novo endereço não foi salvo ({error})"));
    }
    Ok(endpoint)
}

fn describe_error(error: GatewayError) -> String {
    match error {
        GatewayError::Unreachable => {
            "a lâmpada não respondeu na rede (ela está ligada no interruptor?)".to_owned()
        }
        GatewayError::KeyRejected => "a lâmpada recusou a chave local (ela foi pareada de novo? \
                                      é preciso buscar a chave nova)"
            .to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgin_application::config::AppConfig;
    use elgin_application::config::CloudSession;
    use elgin_application::credentials::LocalKey;
    use elgin_application::ports::{CloudDevice, CloudDevices, CloudError, QrLogin};
    use elgin_application::testing::{
        FakeBulbGateway, FakeConfigRepository, ScriptedEvents, StubCloudAccount, StubConnector,
        StubDiscovery,
    };
    use elgin_domain::{
        Brightness, BulbCommand, BulbId, BulbName, ColorTemperature, Hsv, Hue, Saturation,
    };
    use std::net::{IpAddr, Ipv4Addr};

    #[derive(Debug, Default)]
    struct Recorded(Vec<String>);

    impl Output for Recorded {
        fn line(&mut self, text: &str) {
            self.0.push(text.to_owned());
        }
    }

    fn white(brightness: u8, temperature: u8) -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::White {
                brightness: Brightness::from_percent(brightness).unwrap(),
                temperature: ColorTemperature::from_percent(temperature).unwrap(),
            },
        }
    }

    fn endpoint(last: u8) -> Endpoint {
        Endpoint {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, last)),
            protocol_version: "3.5".to_owned(),
        }
    }

    fn bulb(endpoint: Option<Endpoint>) -> BulbConfig {
        BulbConfig {
            id: BulbId::try_new("lampada-1").unwrap(),
            name: BulbName::try_new("Quarto").unwrap(),
            local_key: LocalKey::new("0123456789abcdef"),
            endpoint,
        }
    }

    struct Fixture {
        settings: Settings<FakeConfigRepository>,
        repository: FakeConfigRepository,
        connector: StubConnector,
        discovery: StubDiscovery,
        cloud: StubCloudAccount,
        out: Recorded,
    }

    fn fixture(bulbs: Vec<BulbConfig>) -> Fixture {
        let repository = FakeConfigRepository::with(AppConfig {
            bulbs,
            ..AppConfig::default()
        });
        Fixture {
            settings: Settings::load(repository.clone()).unwrap(),
            repository,
            connector: StubConnector::new(),
            discovery: StubDiscovery::new(),
            cloud: StubCloudAccount::new(),
            out: Recorded::default(),
        }
    }

    impl Fixture {
        async fn run(&mut self, command: CtlCommand) -> Result<(), String> {
            run(
                command,
                &mut self.settings,
                &mut self.connector,
                &mut self.discovery,
                &mut self.cloud,
                &mut self.out,
            )
            .await
        }
    }

    #[test]
    fn describes_white_colour_scene_and_off() {
        assert_eq!(
            describe_state(&white(65, 0)),
            "ligada, branco, brilho 65%, temperatura 0% (quente)"
        );
        assert_eq!(
            describe_state(&white(100, 100)),
            "ligada, branco, brilho 100%, temperatura 100% (fria)"
        );
        let blue = BulbState {
            power: Power::On,
            mode: LightMode::Colour(Hsv::new(
                Hue::new(240),
                Saturation::try_from_permille(800).unwrap(),
                Brightness::from_percent(50).unwrap(),
            )),
        };
        assert_eq!(
            describe_state(&blue),
            "ligada, cor (matiz 240°, saturação 80%), brilho 50%"
        );
        let off_scene = BulbState {
            power: Power::Off,
            mode: LightMode::Scene,
        };
        assert_eq!(describe_state(&off_scene), "desligada, cena");
        let music = BulbState {
            power: Power::On,
            mode: LightMode::Music,
        };
        assert_eq!(describe_state(&music), "ligada, música");
    }

    #[tokio::test]
    async fn status_prints_the_state() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        f.connector.succeed_with(FakeBulbGateway::new(white(65, 0)));

        f.run(CtlCommand::Status).await.unwrap();

        assert_eq!(
            f.out.0,
            ["Quarto: ligada, branco, brilho 65%, temperatura 0% (quente)"]
        );
        assert_eq!(f.connector.attempts()[0].endpoint, endpoint(10));
    }

    #[tokio::test]
    async fn without_a_configured_bulb_explains_what_to_do() {
        let mut f = fixture(Vec::new());

        let error = f.run(CtlCommand::Status).await.unwrap_err();

        assert!(error.contains("nenhuma lâmpada configurada"), "{error}");
    }

    #[tokio::test]
    async fn without_a_saved_address_searches_and_saves_it() {
        let mut f = fixture(vec![bulb(None)]);
        f.discovery.answer(Some(endpoint(42)));
        f.connector.succeed_with(FakeBulbGateway::new(white(65, 0)));

        f.run(CtlCommand::Status).await.unwrap();

        assert_eq!(f.connector.attempts()[0].endpoint, endpoint(42));
        assert_eq!(
            f.repository.saved().unwrap().bulbs[0].endpoint,
            Some(endpoint(42))
        );
    }

    #[tokio::test]
    async fn a_bulb_not_found_on_the_network_is_an_error() {
        let mut f = fixture(vec![bulb(None)]);

        let error = f.run(CtlCommand::Status).await.unwrap_err();

        assert!(error.contains("não foi encontrada na rede"), "{error}");
        assert!(f.connector.attempts().is_empty());
    }

    #[tokio::test]
    async fn an_unreachable_saved_address_is_searched_again() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        f.connector.fail_with(GatewayError::Unreachable);
        f.discovery.answer(Some(endpoint(77)));
        f.connector.succeed_with(FakeBulbGateway::new(white(65, 0)));

        f.run(CtlCommand::Status).await.unwrap();

        assert_eq!(f.connector.attempts()[1].endpoint, endpoint(77));
        assert_eq!(
            f.repository.saved().unwrap().bulbs[0].endpoint,
            Some(endpoint(77))
        );
    }

    #[tokio::test]
    async fn an_unreachable_bulb_still_at_the_same_address_is_offline() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        f.connector.fail_with(GatewayError::Unreachable);
        f.discovery.answer(Some(endpoint(10)));

        let error = f.run(CtlCommand::Status).await.unwrap_err();

        assert!(error.contains("não respondeu"), "{error}");
        assert_eq!(f.connector.attempts().len(), 1);
    }

    #[tokio::test]
    async fn a_rejected_key_is_explained() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        f.connector.fail_with(GatewayError::KeyRejected);

        let error = f.run(CtlCommand::Status).await.unwrap_err();

        assert!(error.contains("recusou a chave"), "{error}");
    }

    #[tokio::test]
    async fn a_command_applies_the_expected_state_and_prints_it() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        let lamp = FakeBulbGateway::new(white(65, 0));
        f.connector.succeed_with(lamp.clone());

        f.run(CtlCommand::Apply(BulbCommand::SetBrightness(
            Brightness::from_percent(20).unwrap(),
        )))
        .await
        .unwrap();

        assert_eq!(lamp.applied(), [white(20, 0)]);
        assert_eq!(
            f.out.0,
            ["Quarto: ligada, branco, brilho 20%, temperatura 0% (quente)"]
        );
    }

    #[tokio::test]
    async fn a_failed_command_is_an_error() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        let lamp = FakeBulbGateway::new(white(65, 0));
        f.connector.succeed_with(lamp.clone());
        lamp.fail_next(GatewayError::Timeout);

        let error = f
            .run(CtlCommand::Apply(BulbCommand::TogglePower))
            .await
            .unwrap_err();

        assert!(error.contains("demorou demais"), "{error}");
    }

    #[tokio::test]
    async fn discover_prints_the_address() {
        let mut f = fixture(vec![bulb(None)]);
        f.discovery.answer(Some(endpoint(42)));

        f.run(CtlCommand::Discover).await.unwrap();

        assert_eq!(f.out.0, ["Quarto: 192.168.0.42 (protocolo 3.5)"]);
        assert_eq!(
            f.repository.saved().unwrap().bulbs[0].endpoint,
            Some(endpoint(42))
        );
    }

    #[tokio::test]
    async fn watch_prints_every_change_until_the_connection_is_lost() {
        let mut f = fixture(vec![bulb(Some(endpoint(10)))]);
        let events = ScriptedEvents::new();
        events.push(BulbEvent::StateReported(white(30, 0)));
        events.push(BulbEvent::StateReported(white(30, 100)));
        events.push(BulbEvent::ConnectionLost(GatewayError::Disconnected));
        f.connector
            .succeed_with_events(FakeBulbGateway::new(white(65, 0)), events);

        let error = f.run(CtlCommand::Watch).await.unwrap_err();

        assert_eq!(
            f.out.0,
            [
                "Quarto: ligada, branco, brilho 65%, temperatura 0% (quente)",
                "Quarto: ligada, branco, brilho 30%, temperatura 0% (quente)",
                "Quarto: ligada, branco, brilho 30%, temperatura 100% (fria)",
            ]
        );
        assert!(error.contains("caiu"), "{error}");
    }

    fn qr_login() -> QrLogin {
        QrLogin {
            user_code: "Abc1234".to_owned(),
            token: "token".to_owned(),
            qr_content: "tuyaSmart--qrLogin?token=token".to_owned(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn login_shows_the_qr_and_saves_the_session_once_confirmed() {
        let mut f = fixture(Vec::new());
        f.cloud.start_returns(Ok(qr_login()));
        f.cloud.poll_returns(Ok(LoginPoll::Pending));
        f.cloud
            .poll_returns(Ok(LoginPoll::Done(CloudSession::new("sessao"))));

        f.run(CtlCommand::Login("Abc1234".to_owned()))
            .await
            .unwrap();

        assert_eq!(f.cloud.user_codes(), ["Abc1234"]);
        assert_eq!(f.cloud.poll_count(), 2);
        assert_eq!(
            f.repository.saved().unwrap().cloud_session,
            Some(CloudSession::new("sessao"))
        );
        assert!(
            f.out.0[1].contains('█') || f.out.0[1].contains('▀'),
            "QR desenhado: {}",
            f.out.0[1]
        );
        assert!(f.out.0.last().unwrap().contains("Login confirmado"));
    }

    #[tokio::test(start_paused = true)]
    async fn login_with_an_invalid_user_code_shows_the_reason() {
        let mut f = fixture(Vec::new());
        f.cloud
            .start_returns(Err(CloudError::Rejected("user code invalid".to_owned())));

        let error = f
            .run(CtlCommand::Login("errado1".to_owned()))
            .await
            .unwrap_err();

        assert!(error.contains("user code invalid"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn login_gives_up_when_the_qr_is_not_confirmed_in_time() {
        let mut f = fixture(Vec::new());
        f.cloud.start_returns(Ok(qr_login()));

        let error = f
            .run(CtlCommand::Login("Abc1234".to_owned()))
            .await
            .unwrap_err();

        assert!(error.contains("não foi confirmado a tempo"), "{error}");
        assert_eq!(f.repository.saved().unwrap().cloud_session, None);
    }

    #[tokio::test]
    async fn devices_lists_the_cloud_devices_and_saves_the_renewed_session() {
        let mut f = fixture(Vec::new());
        f.settings
            .set_cloud_session(Some(CloudSession::new("velha")))
            .unwrap();
        f.cloud.list_returns(Ok(CloudDevices {
            devices: vec![CloudDevice {
                id: "bf123".to_owned(),
                name: "Lâmpada do quarto".to_owned(),
                local_key: LocalKey::new("abcdefghijklmnop"),
                category: "dj".to_owned(),
                is_light: true,
                product_name: "10W Smart color".to_owned(),
            }],
            session: CloudSession::new("nova"),
        }));

        f.run(CtlCommand::Devices).await.unwrap();

        assert_eq!(f.cloud.listed_sessions(), [CloudSession::new("velha")]);
        assert_eq!(
            f.repository.saved().unwrap().cloud_session,
            Some(CloudSession::new("nova"))
        );
        assert_eq!(
            f.out.0,
            ["Lâmpada do quarto (dj, 10W Smart color): id bf123, chave abcd…"]
        );
    }

    #[tokio::test]
    async fn devices_without_a_session_asks_for_the_login() {
        let mut f = fixture(Vec::new());

        let error = f.run(CtlCommand::Devices).await.unwrap_err();

        assert!(error.contains("ctl login"), "{error}");
    }

    #[tokio::test]
    async fn devices_with_an_expired_session_asks_for_the_login_again() {
        let mut f = fixture(Vec::new());
        f.settings
            .set_cloud_session(Some(CloudSession::new("velha")))
            .unwrap();
        f.cloud.list_returns(Err(CloudError::SessionExpired));

        let error = f.run(CtlCommand::Devices).await.unwrap_err();

        assert!(error.contains("expirou"), "{error}");
    }
}
