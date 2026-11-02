//! Mensagens entre o painel e o daemon, com DTOs próprios (o protocolo não depende do formato
//! interno dos tipos do domínio).
//!
//! Conversa: o painel conecta e manda `Hello`; o daemon responde com um `Snapshot` (ou `Shutdown`, se
//! as versões do protocolo não batem) e manda um `Snapshot` novo a cada mudança. O painel manda
//! comandos, favoritas e passos da configuração inicial.

use elgin_application::setup::{SetupDevice, SetupState};
use elgin_application::status::{BulbSnapshot, ConnectionStatus};
use elgin_domain::{
    Brightness, BulbCommand, BulbState, ColorTemperature, FavoriteColour, Favorites, Hsv, Hue,
    LightMode, Power, Saturation,
};
use serde::{Deserialize, Serialize};

/// Versão do protocolo. Muda quando uma mensagem muda de formato.
pub const PROTOCOL_VERSION: u32 = 1;

/// Se o painel que mandou `Hello { protocol_version }` fala o mesmo protocolo deste daemon. Um painel
/// antigo, aberto antes de uma atualização, recebe `Shutdown`.
pub fn is_compatible(protocol_version: u32) -> bool {
    protocol_version == PROTOCOL_VERSION
}

/// Painel → daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelMessage {
    Hello {
        protocol_version: u32,
    },
    Command {
        command: CommandDto,
    },
    ApplyFavorite {
        slot: u8,
    },
    /// Guarda a cor atual na posição `slot`.
    SaveFavorite {
        slot: u8,
    },
    Setup {
        request: SetupRequestDto,
    },
    /// O painel vai fechar.
    Close,
}

/// Daemon → painel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonMessage {
    Snapshot(SnapshotDto),
    /// O painel deve fechar (o daemon vai sair/atualizar, ou o protocolo é outro).
    Shutdown,
}

/// Tudo que o painel mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDto {
    pub bulb_name: Option<String>,
    pub connection: ConnectionDto,
    pub state: Option<StateDto>,
    pub favorites: Vec<FavoriteDto>,
    /// Presente enquanto a configuração inicial está aberta.
    pub setup: Option<SetupStateDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionDto {
    NotConfigured,
    Connecting,
    Online,
    Offline,
    KeyRejected,
    NeedsReconfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDto {
    pub on: bool,
    pub mode: ModeDto,
}

/// Valores em permil (brilho 10–1000, temperatura e saturação 0–1000) e matiz em graus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ModeDto {
    White {
        brightness: u16,
        temperature: u16,
    },
    Colour {
        hue: u16,
        saturation: u16,
        value: u16,
    },
    Scene,
    Music,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteDto {
    pub hue: u16,
    pub saturation: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum CommandDto {
    SetPower {
        on: bool,
    },
    TogglePower,
    SetColour {
        hue: u16,
        saturation: u16,
        value: u16,
    },
    SetWhite {
        brightness: u16,
        temperature: u16,
    },
    SetTemperature {
        temperature: u16,
    },
    SetBrightness {
        brightness: u16,
    },
    StepBrightness {
        percent: i8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum SetupRequestDto {
    /// Abre a configuração inicial (sem mudar a lâmpada atual até o fim).
    Open,
    SubmitUserCode {
        user_code: String,
    },
    Choose {
        device_id: String,
    },
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum SetupStateDto {
    AwaitingUserCode,
    WaitingConfirmation {
        qr_content: String,
    },
    ChooseDevice {
        devices: Vec<SetupDeviceDto>,
    },
    Discovering,
    Done {
        bulb_name: String,
        found_on_network: bool,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupDeviceDto {
    pub id: String,
    pub name: String,
    pub product_name: String,
}

// --- Conversões ---

impl SnapshotDto {
    /// Monta o que o painel mostra a partir do estado do daemon.
    pub fn new(
        bulb_name: Option<&str>,
        snapshot: &BulbSnapshot,
        favorites: &Favorites,
        setup: Option<&SetupState>,
    ) -> Self {
        Self {
            bulb_name: bulb_name.map(str::to_owned),
            connection: snapshot.connection.into(),
            state: snapshot.state.as_ref().map(StateDto::from),
            favorites: favorites
                .colours()
                .iter()
                .map(|&colour| colour.into())
                .collect(),
            setup: setup.map(SetupStateDto::from),
        }
    }
}

impl From<ConnectionStatus> for ConnectionDto {
    fn from(status: ConnectionStatus) -> Self {
        match status {
            ConnectionStatus::NotConfigured => Self::NotConfigured,
            ConnectionStatus::Connecting => Self::Connecting,
            ConnectionStatus::Online => Self::Online,
            ConnectionStatus::Offline => Self::Offline,
            ConnectionStatus::KeyRejected => Self::KeyRejected,
            ConnectionStatus::NeedsReconfiguration => Self::NeedsReconfiguration,
        }
    }
}

impl From<&BulbState> for StateDto {
    fn from(state: &BulbState) -> Self {
        let mode = match state.mode {
            LightMode::White {
                brightness,
                temperature,
            } => ModeDto::White {
                brightness: brightness.permille(),
                temperature: temperature.permille(),
            },
            LightMode::Colour(hsv) => ModeDto::Colour {
                hue: hsv.hue().degrees(),
                saturation: hsv.saturation().permille(),
                value: hsv.value().permille(),
            },
            LightMode::Scene => ModeDto::Scene,
            LightMode::Music => ModeDto::Music,
        };
        Self {
            on: state.power.is_on(),
            mode,
        }
    }
}

impl StateDto {
    /// Volta para o domínio, limitando valores fora da faixa.
    pub fn to_state(self) -> BulbState {
        let mode = match self.mode {
            ModeDto::White {
                brightness,
                temperature,
            } => LightMode::White {
                brightness: Brightness::clamped_from_permille(brightness),
                temperature: ColorTemperature::clamped_from_permille(temperature),
            },
            ModeDto::Colour {
                hue,
                saturation,
                value,
            } => LightMode::Colour(hsv(hue, saturation, value)),
            ModeDto::Scene => LightMode::Scene,
            ModeDto::Music => LightMode::Music,
        };
        BulbState {
            power: power(self.on),
            mode,
        }
    }
}

fn hsv(hue: u16, saturation: u16, value: u16) -> Hsv {
    Hsv::new(
        Hue::new(hue),
        Saturation::clamped_from_permille(saturation),
        Brightness::clamped_from_permille(value),
    )
}

fn power(on: bool) -> Power {
    if on { Power::On } else { Power::Off }
}

impl From<&BulbCommand> for CommandDto {
    fn from(command: &BulbCommand) -> Self {
        match *command {
            BulbCommand::SetPower(power) => Self::SetPower { on: power.is_on() },
            BulbCommand::TogglePower => Self::TogglePower,
            BulbCommand::SetColour(hsv) => Self::SetColour {
                hue: hsv.hue().degrees(),
                saturation: hsv.saturation().permille(),
                value: hsv.value().permille(),
            },
            BulbCommand::SetWhite {
                brightness,
                temperature,
            } => Self::SetWhite {
                brightness: brightness.permille(),
                temperature: temperature.permille(),
            },
            BulbCommand::SetTemperature(temperature) => Self::SetTemperature {
                temperature: temperature.permille(),
            },
            BulbCommand::SetBrightness(brightness) => Self::SetBrightness {
                brightness: brightness.permille(),
            },
            BulbCommand::StepBrightness(percent) => Self::StepBrightness { percent },
        }
    }
}

impl CommandDto {
    /// Volta para o domínio, limitando valores fora da faixa (o painel pode mandar qualquer número).
    pub fn to_command(self) -> BulbCommand {
        match self {
            Self::SetPower { on } => BulbCommand::SetPower(power(on)),
            Self::TogglePower => BulbCommand::TogglePower,
            Self::SetColour {
                hue,
                saturation,
                value,
            } => BulbCommand::SetColour(hsv(hue, saturation, value)),
            Self::SetWhite {
                brightness,
                temperature,
            } => BulbCommand::SetWhite {
                brightness: Brightness::clamped_from_permille(brightness),
                temperature: ColorTemperature::clamped_from_permille(temperature),
            },
            Self::SetTemperature { temperature } => {
                BulbCommand::SetTemperature(ColorTemperature::clamped_from_permille(temperature))
            }
            Self::SetBrightness { brightness } => {
                BulbCommand::SetBrightness(Brightness::clamped_from_permille(brightness))
            }
            Self::StepBrightness { percent } => BulbCommand::StepBrightness(percent),
        }
    }
}

impl From<FavoriteColour> for FavoriteDto {
    fn from(colour: FavoriteColour) -> Self {
        Self {
            hue: colour.hue().degrees(),
            saturation: colour.saturation().permille(),
        }
    }
}

impl FavoriteDto {
    pub fn to_colour(self) -> FavoriteColour {
        FavoriteColour::new(
            Hue::new(self.hue),
            Saturation::clamped_from_permille(self.saturation),
        )
    }
}

impl From<&SetupState> for SetupStateDto {
    fn from(state: &SetupState) -> Self {
        match state {
            SetupState::AwaitingUserCode => Self::AwaitingUserCode,
            SetupState::WaitingConfirmation { qr_content } => Self::WaitingConfirmation {
                qr_content: qr_content.clone(),
            },
            SetupState::ChooseDevice { devices } => Self::ChooseDevice {
                devices: devices.iter().map(SetupDeviceDto::from).collect(),
            },
            SetupState::Discovering => Self::Discovering,
            SetupState::Done {
                bulb,
                found_on_network,
            } => Self::Done {
                bulb_name: bulb.as_str().to_owned(),
                found_on_network: *found_on_network,
            },
            SetupState::Failed { reason } => Self::Failed {
                reason: reason.clone(),
            },
        }
    }
}

impl From<&SetupDevice> for SetupDeviceDto {
    fn from(device: &SetupDevice) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            product_name: device.product_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgin_domain::BulbName;
    use serde_json::json;

    fn colour(hue: u16, saturation: u16, value: u16) -> Hsv {
        Hsv::new(
            Hue::new(hue),
            Saturation::try_from_permille(saturation).unwrap(),
            Brightness::try_from_permille(value).unwrap(),
        )
    }

    fn white_state() -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::White {
                brightness: Brightness::try_from_permille(650).unwrap(),
                temperature: ColorTemperature::try_from_permille(300).unwrap(),
            },
        }
    }

    #[test]
    fn only_the_same_protocol_version_is_compatible() {
        assert!(is_compatible(PROTOCOL_VERSION));
        assert!(!is_compatible(PROTOCOL_VERSION + 1));
        assert!(!is_compatible(0));
    }

    #[test]
    fn panel_messages_have_a_readable_json_format() {
        let message = PanelMessage::Command {
            command: CommandDto::SetBrightness { brightness: 500 },
        };

        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({"type": "command", "command": {"command": "set_brightness", "brightness": 500}})
        );
        assert_eq!(
            serde_json::to_value(PanelMessage::Hello {
                protocol_version: 1
            })
            .unwrap(),
            json!({"type": "hello", "protocol_version": 1})
        );
    }

    #[test]
    fn every_message_round_trips() {
        let panel = [
            PanelMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
            PanelMessage::Command {
                command: CommandDto::StepBrightness { percent: -10 },
            },
            PanelMessage::ApplyFavorite { slot: 2 },
            PanelMessage::SaveFavorite { slot: 5 },
            PanelMessage::Setup {
                request: SetupRequestDto::SubmitUserCode {
                    user_code: "Abc1234".to_owned(),
                },
            },
            PanelMessage::Setup {
                request: SetupRequestDto::Choose {
                    device_id: "bf1".to_owned(),
                },
            },
            PanelMessage::Setup {
                request: SetupRequestDto::Open,
            },
            PanelMessage::Setup {
                request: SetupRequestDto::Cancel,
            },
            PanelMessage::Close,
        ];
        for message in panel {
            let text = serde_json::to_string(&message).unwrap();
            assert_eq!(
                serde_json::from_str::<PanelMessage>(&text).unwrap(),
                message,
                "{text}"
            );
        }

        let snapshot = DaemonMessage::Snapshot(SnapshotDto {
            bulb_name: Some("Quarto".to_owned()),
            connection: ConnectionDto::Online,
            state: Some(StateDto::from(&white_state())),
            favorites: vec![FavoriteDto {
                hue: 0,
                saturation: 1000,
            }],
            setup: Some(SetupStateDto::ChooseDevice {
                devices: vec![SetupDeviceDto {
                    id: "bf1".to_owned(),
                    name: "Quarto".to_owned(),
                    product_name: "10W".to_owned(),
                }],
            }),
        });
        for message in [snapshot, DaemonMessage::Shutdown] {
            let text = serde_json::to_string(&message).unwrap();
            assert_eq!(
                serde_json::from_str::<DaemonMessage>(&text).unwrap(),
                message,
                "{text}"
            );
        }
    }

    #[test]
    fn states_convert_both_ways() {
        let states = [
            white_state(),
            BulbState {
                power: Power::Off,
                mode: LightMode::Colour(colour(240, 800, 500)),
            },
            BulbState {
                power: Power::On,
                mode: LightMode::Scene,
            },
            BulbState {
                power: Power::On,
                mode: LightMode::Music,
            },
        ];
        for state in states {
            assert_eq!(StateDto::from(&state).to_state(), state);
        }
    }

    #[test]
    fn out_of_range_values_from_the_panel_are_clamped() {
        let state = StateDto {
            on: true,
            mode: ModeDto::Colour {
                hue: 400,
                saturation: 5000,
                value: 0,
            },
        }
        .to_state();

        assert_eq!(state.mode, LightMode::Colour(colour(40, 1000, 10)));
        assert_eq!(
            CommandDto::SetBrightness { brightness: 0 }.to_command(),
            BulbCommand::SetBrightness(Brightness::MIN)
        );
        assert_eq!(
            CommandDto::SetWhite {
                brightness: 9000,
                temperature: 9000
            }
            .to_command(),
            BulbCommand::SetWhite {
                brightness: Brightness::MAX,
                temperature: ColorTemperature::COOLEST
            }
        );
    }

    #[test]
    fn commands_convert_both_ways() {
        let commands = [
            BulbCommand::SetPower(Power::On),
            BulbCommand::SetPower(Power::Off),
            BulbCommand::TogglePower,
            BulbCommand::SetColour(colour(120, 700, 300)),
            BulbCommand::SetWhite {
                brightness: Brightness::try_from_permille(200).unwrap(),
                temperature: ColorTemperature::WARMEST,
            },
            BulbCommand::SetTemperature(ColorTemperature::COOLEST),
            BulbCommand::SetBrightness(Brightness::try_from_permille(420).unwrap()),
            BulbCommand::StepBrightness(-5),
        ];
        for command in commands {
            assert_eq!(CommandDto::from(&command).to_command(), command);
        }
    }

    #[test]
    fn favorites_convert_both_ways() {
        let colour =
            FavoriteColour::new(Hue::new(170), Saturation::try_from_permille(600).unwrap());

        assert_eq!(FavoriteDto::from(colour).to_colour(), colour);
    }

    #[test]
    fn the_snapshot_collects_everything_the_panel_shows() {
        let snapshot = BulbSnapshot {
            connection: ConnectionStatus::Offline,
            state: Some(white_state()),
        };
        let setup = SetupState::Done {
            bulb: BulbName::try_new("Quarto").unwrap(),
            found_on_network: false,
        };

        let dto = SnapshotDto::new(
            Some("Quarto"),
            &snapshot,
            &Favorites::default(),
            Some(&setup),
        );

        assert_eq!(dto.bulb_name.as_deref(), Some("Quarto"));
        assert_eq!(dto.connection, ConnectionDto::Offline);
        assert_eq!(dto.state, Some(StateDto::from(&white_state())));
        assert_eq!(dto.favorites.len(), Favorites::COUNT);
        assert_eq!(
            dto.favorites[1],
            FavoriteDto {
                hue: 30,
                saturation: 1000
            }
        );
        assert_eq!(
            dto.setup,
            Some(SetupStateDto::Done {
                bulb_name: "Quarto".to_owned(),
                found_on_network: false
            })
        );
    }

    #[test]
    fn connection_statuses_map_one_to_one() {
        let pairs = [
            (
                ConnectionStatus::NotConfigured,
                ConnectionDto::NotConfigured,
            ),
            (ConnectionStatus::Connecting, ConnectionDto::Connecting),
            (ConnectionStatus::Online, ConnectionDto::Online),
            (ConnectionStatus::Offline, ConnectionDto::Offline),
            (ConnectionStatus::KeyRejected, ConnectionDto::KeyRejected),
            (
                ConnectionStatus::NeedsReconfiguration,
                ConnectionDto::NeedsReconfiguration,
            ),
        ];
        for (status, dto) in pairs {
            assert_eq!(ConnectionDto::from(status), dto);
        }
    }

    #[test]
    fn setup_states_are_converted() {
        let devices = vec![SetupDevice {
            id: "bf1".to_owned(),
            name: "Quarto".to_owned(),
            product_name: "10W".to_owned(),
        }];

        assert_eq!(
            SetupStateDto::from(&SetupState::AwaitingUserCode),
            SetupStateDto::AwaitingUserCode
        );
        assert_eq!(
            SetupStateDto::from(&SetupState::WaitingConfirmation {
                qr_content: "qr".to_owned()
            }),
            SetupStateDto::WaitingConfirmation {
                qr_content: "qr".to_owned()
            }
        );
        assert_eq!(
            SetupStateDto::from(&SetupState::ChooseDevice { devices }),
            SetupStateDto::ChooseDevice {
                devices: vec![SetupDeviceDto {
                    id: "bf1".to_owned(),
                    name: "Quarto".to_owned(),
                    product_name: "10W".to_owned()
                }]
            }
        );
        assert_eq!(
            SetupStateDto::from(&SetupState::Discovering),
            SetupStateDto::Discovering
        );
        assert_eq!(
            SetupStateDto::from(&SetupState::Failed {
                reason: "x".to_owned()
            }),
            SetupStateDto::Failed {
                reason: "x".to_owned()
            }
        );
    }
}
