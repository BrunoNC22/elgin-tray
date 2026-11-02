//! Configuração persistida do app e os casos de uso que a alteram.

use std::fmt;

use elgin_domain::{BulbId, BulbName, FavoriteColour, FavoriteSlot, Favorites};

use crate::credentials::{Endpoint, LocalKey};
use crate::ports::{ConfigError, ConfigRepository};

/// Uma lâmpada configurada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulbConfig {
    pub id: BulbId,
    pub name: BulbName,
    pub local_key: LocalKey,
    /// Último endereço conhecido (`None`: procurar na rede).
    pub endpoint: Option<Endpoint>,
}

/// Atalhos globais, como texto (ex.: `Ctrl+Alt+L`). O parsing fica com quem registra os atalhos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBindings {
    pub toggle: String,
    pub brightness_up: String,
    pub brightness_down: String,
    pub next_favorite: String,
}

impl Default for HotkeyBindings {
    fn default() -> Self {
        Self {
            toggle: "Ctrl+Alt+L".to_owned(),
            brightness_up: "Ctrl+Alt+Up".to_owned(),
            brightness_down: "Ctrl+Alt+Down".to_owned(),
            next_favorite: "Ctrl+Alt+Right".to_owned(),
        }
    }
}

/// Sessão da nuvem, guardada para recuperar a local key sozinho. Segredo.
///
/// O conteúdo é opaco para a aplicação: o adaptador de nuvem serializa e interpreta.
#[derive(Clone, PartialEq, Eq)]
pub struct CloudSession(String);

impl CloudSession {
    pub fn new(data: impl Into<String>) -> Self {
        Self(data.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CloudSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CloudSession(***)")
    }
}

/// Preferências de atualização.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePreferences {
    /// Última versão nova já avisada ao usuário (para avisar uma vez só).
    pub last_notified_version: Option<String>,
}

/// Tudo que o app guarda entre execuções.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Lâmpadas configuradas. A v1.0 usa só a primeira.
    pub bulbs: Vec<BulbConfig>,
    pub favorites: Favorites,
    pub hotkeys: HotkeyBindings,
    /// Iniciar com o sistema. `None`: o usuário nunca escolheu (a primeira execução ativa).
    pub autostart: Option<bool>,
    pub cloud_session: Option<CloudSession>,
    pub updates: UpdatePreferences,
}

impl AppConfig {
    /// A lâmpada controlada pela v1.0.
    pub fn primary_bulb(&self) -> Option<&BulbConfig> {
        self.bulbs.first()
    }
}

/// A configuração em memória mais o repositório onde ela é salva.
///
/// Cada alteração é salva na hora. Se salvar falhar, a configuração em memória continua como
/// estava (nada fica "meio salvo").
#[derive(Debug)]
pub struct Settings<R> {
    repository: R,
    config: AppConfig,
}

impl<R: ConfigRepository> Settings<R> {
    /// Carrega do repositório; sem arquivo ainda, começa com a configuração padrão (sem salvar).
    pub fn load(repository: R) -> Result<Self, ConfigError> {
        let config = repository.load()?.unwrap_or_default();
        Ok(Self { repository, config })
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Guarda uma cor favorita.
    pub fn save_favorite(
        &mut self,
        slot: FavoriteSlot,
        colour: FavoriteColour,
    ) -> Result<(), ConfigError> {
        self.update(|config| config.favorites.save(slot, colour))
    }

    /// Guarda o endereço novo de uma lâmpada. Não salva nada se a lâmpada não existe ou o endereço
    /// é o mesmo.
    pub fn update_endpoint(&mut self, id: &BulbId, endpoint: Endpoint) -> Result<(), ConfigError> {
        self.update(|config| {
            if let Some(bulb) = config.bulbs.iter_mut().find(|bulb| &bulb.id == id) {
                bulb.endpoint = Some(endpoint);
            }
        })
    }

    /// Guarda a escolha do usuário sobre iniciar com o sistema.
    pub fn set_autostart(&mut self, enabled: bool) -> Result<(), ConfigError> {
        self.update(|config| config.autostart = Some(enabled))
    }

    /// Coloca a lâmpada como a principal (configuração inicial ou reconfiguração), substituindo a
    /// lâmpada de mesmo id, se houver.
    pub fn set_primary_bulb(&mut self, bulb: BulbConfig) -> Result<(), ConfigError> {
        self.update(|config| {
            config.bulbs.retain(|existing| existing.id != bulb.id);
            config.bulbs.insert(0, bulb);
        })
    }

    pub fn set_cloud_session(&mut self, session: Option<CloudSession>) -> Result<(), ConfigError> {
        self.update(|config| config.cloud_session = session)
    }

    pub fn mark_version_notified(&mut self, version: &str) -> Result<(), ConfigError> {
        self.update(|config| config.updates.last_notified_version = Some(version.to_owned()))
    }

    /// Aplica `change` numa cópia; se mudou algo, salva e só então troca a configuração em memória.
    fn update(&mut self, change: impl FnOnce(&mut AppConfig)) -> Result<(), ConfigError> {
        let mut updated = self.config.clone();
        change(&mut updated);
        if updated != self.config {
            self.repository.save(&updated)?;
            self.config = updated;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeConfigRepository;
    use elgin_domain::{Hue, Saturation};
    use std::net::{IpAddr, Ipv4Addr};

    fn bulb(id: &str, key: &str) -> BulbConfig {
        BulbConfig {
            id: BulbId::try_new(id).unwrap(),
            name: BulbName::try_new("Lâmpada do quarto").unwrap(),
            local_key: LocalKey::new(key),
            endpoint: None,
        }
    }

    fn endpoint(last_octet: u8) -> Endpoint {
        Endpoint {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, last_octet)),
            protocol_version: "3.5".to_owned(),
        }
    }

    fn teal() -> FavoriteColour {
        FavoriteColour::new(Hue::new(170), Saturation::FULL)
    }

    fn slot(index: u8) -> FavoriteSlot {
        FavoriteSlot::try_new(index).unwrap()
    }

    fn settings_with(config: AppConfig) -> (Settings<FakeConfigRepository>, FakeConfigRepository) {
        let repository = FakeConfigRepository::with(config);
        (Settings::load(repository.clone()).unwrap(), repository)
    }

    #[test]
    fn default_hotkeys() {
        let hotkeys = HotkeyBindings::default();

        assert_eq!(hotkeys.toggle, "Ctrl+Alt+L");
        assert_eq!(hotkeys.brightness_up, "Ctrl+Alt+Up");
        assert_eq!(hotkeys.brightness_down, "Ctrl+Alt+Down");
        assert_eq!(hotkeys.next_favorite, "Ctrl+Alt+Right");
    }

    #[test]
    fn secrets_never_appear_in_debug() {
        let config = AppConfig {
            bulbs: vec![bulb("lampada-1", "chave-secreta-123")],
            cloud_session: Some(CloudSession::new("token-secreto-456")),
            ..AppConfig::default()
        };

        let debug = format!("{config:?}");

        assert!(!debug.contains("chave-secreta-123"), "{debug}");
        assert!(!debug.contains("token-secreto-456"), "{debug}");
        assert!(debug.contains("LocalKey(***)"));
        assert!(debug.contains("CloudSession(***)"));
    }

    #[test]
    fn primary_bulb_is_the_first() {
        let config = AppConfig {
            bulbs: vec![bulb("a", "k"), bulb("b", "k")],
            ..AppConfig::default()
        };

        assert_eq!(config.primary_bulb().unwrap().id.as_str(), "a");
        assert_eq!(AppConfig::default().primary_bulb(), None);
    }

    #[test]
    fn loading_without_a_saved_file_starts_with_defaults_without_saving() {
        let repository = FakeConfigRepository::empty();

        let settings = Settings::load(repository.clone()).unwrap();

        assert_eq!(settings.config(), &AppConfig::default());
        assert_eq!(repository.save_count(), 0);
    }

    #[test]
    fn loading_returns_the_saved_config() {
        let saved = AppConfig {
            autostart: Some(false),
            ..AppConfig::default()
        };

        let (settings, _) = settings_with(saved.clone());

        assert_eq!(settings.config(), &saved);
    }

    #[test]
    fn loading_fails_when_the_repository_fails() {
        let repository = FakeConfigRepository::empty();
        repository.fail_next(ConfigError::Invalid("linha 3".to_owned()));

        assert_eq!(
            Settings::load(repository).unwrap_err(),
            ConfigError::Invalid("linha 3".to_owned())
        );
    }

    #[test]
    fn save_favorite_updates_and_persists() {
        let (mut settings, repository) = settings_with(AppConfig::default());

        settings.save_favorite(slot(2), teal()).unwrap();

        assert_eq!(settings.config().favorites.get(slot(2)), teal());
        assert_eq!(repository.saved().unwrap().favorites.get(slot(2)), teal());
    }

    #[test]
    fn a_failed_save_keeps_the_previous_config() {
        let (mut settings, repository) = settings_with(AppConfig::default());
        repository.fail_next(ConfigError::Io("disco cheio".to_owned()));

        let result = settings.save_favorite(slot(2), teal());

        assert_eq!(result, Err(ConfigError::Io("disco cheio".to_owned())));
        assert_eq!(settings.config(), &AppConfig::default());
    }

    #[test]
    fn update_endpoint_saves_the_new_address() {
        let (mut settings, repository) = settings_with(AppConfig {
            bulbs: vec![bulb("lampada-1", "k")],
            ..AppConfig::default()
        });

        settings
            .update_endpoint(&BulbId::try_new("lampada-1").unwrap(), endpoint(42))
            .unwrap();

        assert_eq!(settings.config().bulbs[0].endpoint, Some(endpoint(42)));
        assert_eq!(
            repository.saved().unwrap().bulbs[0].endpoint,
            Some(endpoint(42))
        );
    }

    #[test]
    fn update_endpoint_does_not_save_when_nothing_changes() {
        let mut configured = bulb("lampada-1", "k");
        configured.endpoint = Some(endpoint(42));
        let (mut settings, repository) = settings_with(AppConfig {
            bulbs: vec![configured],
            ..AppConfig::default()
        });

        settings
            .update_endpoint(&BulbId::try_new("lampada-1").unwrap(), endpoint(42))
            .unwrap();
        settings
            .update_endpoint(&BulbId::try_new("outra").unwrap(), endpoint(99))
            .unwrap();

        assert_eq!(repository.save_count(), 0);
    }

    #[test]
    fn set_autostart_records_the_user_choice() {
        let (mut settings, repository) = settings_with(AppConfig::default());

        settings.set_autostart(false).unwrap();

        assert_eq!(repository.saved().unwrap().autostart, Some(false));
    }

    #[test]
    fn set_primary_bulb_replaces_the_bulb_with_the_same_id_and_puts_it_first() {
        let (mut settings, _) = settings_with(AppConfig {
            bulbs: vec![bulb("outra", "k"), bulb("lampada-1", "chave-velha")],
            ..AppConfig::default()
        });

        settings
            .set_primary_bulb(bulb("lampada-1", "chave-nova"))
            .unwrap();

        let bulbs = &settings.config().bulbs;
        assert_eq!(bulbs.len(), 2);
        assert_eq!(bulbs[0].id.as_str(), "lampada-1");
        assert_eq!(bulbs[0].local_key, LocalKey::new("chave-nova"));
        assert_eq!(bulbs[1].id.as_str(), "outra");
    }

    #[test]
    fn cloud_session_and_notified_version_are_saved() {
        let (mut settings, repository) = settings_with(AppConfig::default());

        settings
            .set_cloud_session(Some(CloudSession::new("sessao")))
            .unwrap();
        settings.mark_version_notified("1.2.0").unwrap();

        let saved = repository.saved().unwrap();
        assert_eq!(saved.cloud_session, Some(CloudSession::new("sessao")));
        assert_eq!(
            saved.updates.last_notified_version.as_deref(),
            Some("1.2.0")
        );
    }
}
