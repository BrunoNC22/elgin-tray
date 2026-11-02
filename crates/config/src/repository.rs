//! O `config.toml`: formato do arquivo e a porta `ConfigRepository`.
//!
//! Exemplo do arquivo (todas as seções são opcionais; o que faltar usa o padrão):
//!
//! ```toml
//! version = 1
//! autostart = true
//!
//! [[bulbs]]
//! id = "eb0123456789abcdefgh"
//! name = "Lâmpada do quarto"
//! local_key = "0123456789abcdef"
//! ip = "192.168.0.50"          # opcional: sem ele, a lâmpada é procurada na rede
//! protocol_version = "3.5"     # opcional, junto com o ip
//!
//! [[favorites]]
//! hue = 0                      # 0–359
//! saturation = 1000            # 0–1000
//!
//! [hotkeys]
//! toggle = "Ctrl+Alt+L"
//! brightness_up = "Ctrl+Alt+Up"
//! brightness_down = "Ctrl+Alt+Down"
//! next_favorite = "Ctrl+Alt+Right"
//!
//! [cloud]
//! session = "..."              # gravado pelo app; não editar
//!
//! [updates]
//! last_notified_version = "1.2.0"
//! ```

use std::path::{Path, PathBuf};

use elgin_application::config::{
    AppConfig, BulbConfig, CloudSession, HotkeyBindings, UpdatePreferences,
};
use elgin_application::credentials::{Endpoint, LocalKey};
use elgin_application::ports::{ConfigError, ConfigRepository};
use elgin_domain::{BulbId, BulbName, FavoriteColour, Favorites, Hue, Saturation};
use serde::{Deserialize, Serialize};

use crate::store::{FileStore, FsStore};

/// Versão do formato que este app grava. Arquivos mais novos são recusados (não sabemos ler).
const FORMAT_VERSION: u32 = 1;

/// Configuração guardada num arquivo TOML.
#[derive(Debug, Clone)]
pub struct TomlConfigRepository<F = FsStore> {
    store: F,
    path: PathBuf,
}

impl TomlConfigRepository<FsStore> {
    /// Repositório no local padrão: `%APPDATA%\elgin-tray\config.toml` (Windows) ou
    /// `~/.config/elgin-tray/config.toml` (Linux). `None` se o sistema não informa a pasta.
    pub fn at_default_location() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        Some(Self::new(
            FsStore,
            base.config_dir().join("elgin-tray").join("config.toml"),
        ))
    }
}

impl<F: FileStore> TomlConfigRepository<F> {
    pub fn new(store: F, path: PathBuf) -> Self {
        Self { store, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl<F: FileStore> ConfigRepository for TomlConfigRepository<F> {
    fn load(&self) -> Result<Option<AppConfig>, ConfigError> {
        let Some(text) = self.store.read(&self.path).map_err(io_error)? else {
            return Ok(None);
        };
        let file: FileDto =
            toml::from_str(&text).map_err(|error| ConfigError::Invalid(error.to_string()))?;
        file.into_config().map(Some)
    }

    fn save(&mut self, config: &AppConfig) -> Result<(), ConfigError> {
        let text = toml::to_string(&FileDto::from_config(config))
            .map_err(|error| ConfigError::Invalid(error.to_string()))?;
        self.store.write_atomic(&self.path, &text).map_err(io_error)
    }
}

fn io_error(error: std::io::Error) -> ConfigError {
    ConfigError::Io(error.to_string())
}

fn invalid(context: impl std::fmt::Display, error: impl std::fmt::Display) -> ConfigError {
    ConfigError::Invalid(format!("{context}: {error}"))
}

// Formato do arquivo. A ordem dos campos importa: no TOML, valores simples vêm antes das tabelas.

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct FileDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    autostart: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bulbs: Vec<BulbDto>,
    favorites: Vec<FavoriteDto>,
    hotkeys: HotkeysDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud: Option<CloudDto>,
    updates: UpdatesDto,
}

#[derive(Debug, Serialize, Deserialize)]
struct BulbDto {
    id: String,
    name: String,
    local_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FavoriteDto {
    hue: u16,
    saturation: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct HotkeysDto {
    toggle: String,
    brightness_up: String,
    brightness_down: String,
    next_favorite: String,
}

impl Default for HotkeysDto {
    fn default() -> Self {
        let defaults = HotkeyBindings::default();
        Self {
            toggle: defaults.toggle,
            brightness_up: defaults.brightness_up,
            brightness_down: defaults.brightness_down,
            next_favorite: defaults.next_favorite,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CloudDto {
    session: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct UpdatesDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    last_notified_version: Option<String>,
}

impl FileDto {
    fn from_config(config: &AppConfig) -> Self {
        Self {
            version: Some(FORMAT_VERSION),
            autostart: config.autostart,
            bulbs: config
                .bulbs
                .iter()
                .map(|bulb| BulbDto {
                    id: bulb.id.as_str().to_owned(),
                    name: bulb.name.as_str().to_owned(),
                    local_key: bulb.local_key.expose().to_owned(),
                    ip: bulb
                        .endpoint
                        .as_ref()
                        .map(|endpoint| endpoint.ip.to_string()),
                    protocol_version: bulb
                        .endpoint
                        .as_ref()
                        .map(|endpoint| endpoint.protocol_version.clone()),
                })
                .collect(),
            favorites: config
                .favorites
                .colours()
                .iter()
                .map(|colour| FavoriteDto {
                    hue: colour.hue().degrees(),
                    saturation: colour.saturation().permille(),
                })
                .collect(),
            hotkeys: HotkeysDto {
                toggle: config.hotkeys.toggle.clone(),
                brightness_up: config.hotkeys.brightness_up.clone(),
                brightness_down: config.hotkeys.brightness_down.clone(),
                next_favorite: config.hotkeys.next_favorite.clone(),
            },
            cloud: config.cloud_session.as_ref().map(|session| CloudDto {
                session: session.expose().to_owned(),
            }),
            updates: UpdatesDto {
                last_notified_version: config.updates.last_notified_version.clone(),
            },
        }
    }

    fn into_config(self) -> Result<AppConfig, ConfigError> {
        let version = self.version.unwrap_or(FORMAT_VERSION);
        if version > FORMAT_VERSION {
            return Err(ConfigError::Invalid(format!(
                "o arquivo é da versão {version} do formato, mais nova que a deste app ({FORMAT_VERSION})"
            )));
        }

        let bulbs = self
            .bulbs
            .into_iter()
            .enumerate()
            .map(|(index, bulb)| bulb.into_config(index + 1))
            .collect::<Result<Vec<_>, _>>()?;

        // Favoritas a mais são ignoradas; a menos, completadas com as padrão.
        let mut favorites = Favorites::default();
        for (slot, colour) in elgin_domain::FavoriteSlot::all().zip(self.favorites) {
            favorites.save(
                slot,
                FavoriteColour::new(
                    Hue::new(colour.hue),
                    Saturation::clamped_from_permille(colour.saturation),
                ),
            );
        }

        Ok(AppConfig {
            bulbs,
            favorites,
            hotkeys: HotkeyBindings {
                toggle: self.hotkeys.toggle,
                brightness_up: self.hotkeys.brightness_up,
                brightness_down: self.hotkeys.brightness_down,
                next_favorite: self.hotkeys.next_favorite,
            },
            autostart: self.autostart,
            cloud_session: self.cloud.map(|cloud| CloudSession::new(cloud.session)),
            updates: UpdatePreferences {
                last_notified_version: self.updates.last_notified_version,
            },
        })
    }
}

impl BulbDto {
    fn into_config(self, number: usize) -> Result<BulbConfig, ConfigError> {
        let context = format!("lâmpada {number}");
        // Endereço só com IP e versão juntos; senão a lâmpada é procurada na rede.
        let endpoint = match (self.ip, self.protocol_version) {
            (Some(ip), Some(protocol_version)) => Some(Endpoint {
                ip: ip
                    .parse()
                    .map_err(|error| invalid(format!("{context}, ip \"{ip}\""), error))?,
                protocol_version,
            }),
            _ => None,
        };
        Ok(BulbConfig {
            id: BulbId::try_new(self.id).map_err(|error| invalid(&context, error))?,
            name: BulbName::try_new(self.name).map_err(|error| invalid(&context, error))?,
            local_key: LocalKey::new(self.local_key),
            endpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FakeFileStore;
    use elgin_domain::FavoriteSlot;
    use std::io;
    use std::net::{IpAddr, Ipv4Addr};

    fn path() -> PathBuf {
        PathBuf::from("config/elgin-tray/config.toml")
    }

    fn repository() -> (TomlConfigRepository<FakeFileStore>, FakeFileStore) {
        let store = FakeFileStore::default();
        (TomlConfigRepository::new(store.clone(), path()), store)
    }

    fn full_config() -> AppConfig {
        let mut favorites = Favorites::default();
        favorites.save(
            FavoriteSlot::try_new(3).unwrap(),
            FavoriteColour::new(Hue::new(170), Saturation::try_from_permille(600).unwrap()),
        );
        AppConfig {
            bulbs: vec![
                BulbConfig {
                    id: BulbId::try_new("lampada-1").unwrap(),
                    name: BulbName::try_new("Lâmpada do quarto").unwrap(),
                    local_key: LocalKey::new("0123456789abcdef"),
                    endpoint: Some(Endpoint {
                        ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, 50)),
                        protocol_version: "3.5".to_owned(),
                    }),
                },
                BulbConfig {
                    id: BulbId::try_new("lampada-2").unwrap(),
                    name: BulbName::try_new("Sala").unwrap(),
                    local_key: LocalKey::new("fedcba9876543210"),
                    endpoint: None,
                },
            ],
            favorites,
            hotkeys: HotkeyBindings {
                toggle: "Ctrl+Shift+L".to_owned(),
                ..HotkeyBindings::default()
            },
            autostart: Some(false),
            cloud_session: Some(CloudSession::new("{\"token\":\"abc\"}")),
            updates: UpdatePreferences {
                last_notified_version: Some("1.2.0".to_owned()),
            },
        }
    }

    #[test]
    fn a_missing_file_means_no_config_yet() {
        let (repository, _) = repository();

        assert_eq!(repository.load(), Ok(None));
    }

    #[test]
    fn saving_and_loading_keeps_everything() {
        let (mut repository, _) = repository();

        repository.save(&full_config()).unwrap();

        assert_eq!(repository.load(), Ok(Some(full_config())));
    }

    #[test]
    fn the_default_config_round_trips_too() {
        let (mut repository, _) = repository();

        repository.save(&AppConfig::default()).unwrap();

        assert_eq!(repository.load(), Ok(Some(AppConfig::default())));
    }

    #[test]
    fn the_saved_file_is_readable_toml_with_the_format_version() {
        let (mut repository, store) = repository();

        repository.save(&full_config()).unwrap();

        let text = store.get(&path()).unwrap();
        assert!(text.starts_with("version = 1\n"), "{text}");
        assert!(text.contains("[[bulbs]]"), "{text}");
        assert!(text.contains("local_key = \"0123456789abcdef\""), "{text}");
        assert!(text.contains("ip = \"192.168.0.50\""), "{text}");
    }

    #[test]
    fn a_minimal_hand_written_file_uses_defaults_for_the_rest() {
        let (repository, store) = repository();
        store.put(
            &path(),
            "[[bulbs]]\nid = \"lampada-1\"\nname = \"Quarto\"\nlocal_key = \"0123456789abcdef\"\n",
        );

        let config = repository.load().unwrap().unwrap();

        assert_eq!(config.bulbs.len(), 1);
        assert_eq!(config.bulbs[0].endpoint, None);
        assert_eq!(config.favorites, Favorites::default());
        assert_eq!(config.hotkeys, HotkeyBindings::default());
        assert_eq!(config.autostart, None);
        assert_eq!(config.cloud_session, None);
    }

    #[test]
    fn missing_hotkeys_use_their_defaults() {
        let (repository, store) = repository();
        store.put(&path(), "[hotkeys]\ntoggle = \"Ctrl+Shift+T\"\n");

        let hotkeys = repository.load().unwrap().unwrap().hotkeys;

        assert_eq!(hotkeys.toggle, "Ctrl+Shift+T");
        assert_eq!(
            hotkeys.brightness_up,
            HotkeyBindings::default().brightness_up
        );
    }

    #[test]
    fn an_ip_without_the_protocol_version_is_ignored() {
        let (repository, store) = repository();
        store.put(
            &path(),
            "[[bulbs]]\nid = \"a\"\nname = \"b\"\nlocal_key = \"c\"\nip = \"192.168.0.9\"\n",
        );

        assert_eq!(repository.load().unwrap().unwrap().bulbs[0].endpoint, None);
    }

    #[test]
    fn favorites_are_completed_with_the_defaults_and_values_limited() {
        let (repository, store) = repository();
        store.put(&path(), "[[favorites]]\nhue = 400\nsaturation = 5000\n");

        let favorites = repository.load().unwrap().unwrap().favorites;

        let first = favorites.get(FavoriteSlot::try_new(0).unwrap());
        assert_eq!(first.hue().degrees(), 40);
        assert_eq!(first.saturation(), Saturation::FULL);
        assert_eq!(
            favorites.get(FavoriteSlot::try_new(1).unwrap()),
            Favorites::default().get(FavoriteSlot::try_new(1).unwrap())
        );
    }

    #[test]
    fn a_corrupted_file_is_an_error_and_is_left_untouched() {
        let (repository, store) = repository();
        store.put(&path(), "isto não é [toml");

        let result = repository.load();

        assert!(matches!(result, Err(ConfigError::Invalid(_))), "{result:?}");
        assert_eq!(store.get(&path()).as_deref(), Some("isto não é [toml"));
    }

    #[test]
    fn invalid_values_are_errors() {
        let (repository, store) = repository();
        store.put(
            &path(),
            "[[bulbs]]\nid = \"  \"\nname = \"b\"\nlocal_key = \"c\"\n",
        );
        assert!(matches!(repository.load(), Err(ConfigError::Invalid(_))));

        store.put(
            &path(),
            "[[bulbs]]\nid = \"a\"\nname = \"b\"\nlocal_key = \"c\"\nip = \"não é ip\"\nprotocol_version = \"3.5\"\n",
        );
        assert!(matches!(repository.load(), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn a_file_from_a_newer_version_is_refused() {
        let (repository, store) = repository();
        store.put(&path(), "version = 2\n");

        let result = repository.load();

        assert!(
            matches!(&result, Err(ConfigError::Invalid(message)) if message.contains('2')),
            "{result:?}"
        );
    }

    #[test]
    fn read_and_write_failures_are_io_errors() {
        let (mut repository, store) = repository();

        store.fail_next(io::ErrorKind::PermissionDenied);
        assert!(matches!(repository.load(), Err(ConfigError::Io(_))));

        store.fail_next(io::ErrorKind::StorageFull);
        assert!(matches!(
            repository.save(&AppConfig::default()),
            Err(ConfigError::Io(_))
        ));
    }
}
