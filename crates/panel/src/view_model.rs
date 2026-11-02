//! O view-model do painel: o que a tela mostra, a partir dos snapshots do daemon, e as mensagens que
//! cada gesto do usuário gera. Sem Slint (testável); a tela só lê [`PanelView`] e chama os métodos.
//!
//! Atualização otimista: cada gesto já muda o que é mostrado (pela mesma regra do domínio que o
//! daemon usa), sem esperar o próximo snapshot.

use elgin_domain::{
    Brightness, BulbCommand, BulbState, ColorTemperature, FavoriteColour, Hsv, Hue, LightMode,
    Power, Rgb, Saturation,
};
use elgin_ipc::messages::{
    CommandDto, ConnectionDto, PanelMessage, SetupRequestDto, SetupStateDto, SnapshotDto, StateDto,
};

use crate::disc::{DiscPoint, colour_at, point_of};

/// Aba do painel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Colour,
    White,
}

/// O que a tela mostra.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelView {
    pub bulb_name: String,
    /// Aviso no topo quando a lâmpada não está disponível.
    pub notice: Option<String>,
    /// Controles da lâmpada habilitados (só com a lâmpada online).
    pub controls_enabled: bool,
    pub power_on: bool,
    pub tab: Tab,
    /// Marcador no disco de cores.
    pub marker: DiscPoint,
    pub brightness_percent: u8,
    pub temperature_percent: u8,
    /// Cor "cheia" atual: fim do gradiente do slider de brilho (o início é preto).
    pub full_colour: Rgb,
    /// As 6 cores favoritas, para os botões.
    pub favorites: Vec<Rgb>,
    /// Configuração inicial em andamento (a tela mostra os passos dela no lugar dos controles).
    pub setup: Option<SetupStateDto>,
    /// Mostra o botão "Configurar lâmpada" (sem lâmpada ou precisando reconfigurar).
    pub can_configure: bool,
}

/// O view-model.
#[derive(Debug, Clone)]
pub struct PanelViewModel {
    snapshot: Option<SnapshotDto>,
    /// Estado mostrado (pode estar à frente do snapshot, pela atualização otimista).
    state: Option<BulbState>,
    tab: Tab,
    /// Última cor e última temperatura vistas, para voltar a elas ao trocar de aba.
    last_colour: Hsv,
    last_temperature: ColorTemperature,
}

impl Default for PanelViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl PanelViewModel {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            state: None,
            tab: Tab::White,
            last_colour: Hsv::new(Hue::new(0), Saturation::FULL, Brightness::MAX),
            last_temperature: ColorTemperature::WARMEST,
        }
    }

    /// O daemon mandou um estado novo.
    pub fn on_snapshot(&mut self, snapshot: SnapshotDto) {
        self.state = snapshot.state.map(StateDto::to_state);
        self.remember_mode();
        self.snapshot = Some(snapshot);
    }

    pub fn view(&self) -> PanelView {
        let snapshot = self.snapshot.as_ref();
        let connection = snapshot.map(|snapshot| snapshot.connection);
        let setup = snapshot.and_then(|snapshot| snapshot.setup.clone());
        let can_configure = setup.is_none()
            && matches!(
                connection,
                Some(ConnectionDto::NotConfigured | ConnectionDto::NeedsReconfiguration)
            );
        let notice = match connection {
            None => Some("Conectando ao app…"),
            Some(ConnectionDto::Online) => None,
            Some(ConnectionDto::NotConfigured) => Some("Nenhuma lâmpada configurada."),
            Some(ConnectionDto::Connecting) => Some("Conectando à lâmpada…"),
            Some(ConnectionDto::Offline) => Some("A lâmpada não está respondendo na rede."),
            Some(ConnectionDto::KeyRejected) => {
                Some("A lâmpada recusou a chave. Tentando recuperar…")
            }
            Some(ConnectionDto::NeedsReconfiguration) => {
                Some("É preciso configurar a lâmpada de novo.")
            }
        };
        let mode = self.state.map(|state| state.mode);
        let full_colour = match (mode.and_then(|mode| mode.display_colour()), self.tab) {
            (Some(colour), _) => colour,
            (None, Tab::Colour) => self.last_colour.with_value(Brightness::MAX).to_rgb(),
            (None, Tab::White) => self.last_temperature.display_colour(),
        };
        PanelView {
            bulb_name: snapshot
                .and_then(|snapshot| snapshot.bulb_name.clone())
                .unwrap_or_else(|| "elgin-tray".to_owned()),
            notice: notice.map(str::to_owned),
            controls_enabled: self.is_online(),
            power_on: self.state.is_some_and(|state| state.power.is_on()),
            tab: self.tab,
            marker: point_of(FavoriteColour::new(
                self.last_colour.hue(),
                self.last_colour.saturation(),
            )),
            brightness_percent: self.current_brightness().percent(),
            temperature_percent: self.last_temperature.percent(),
            full_colour,
            favorites: snapshot
                .map(|snapshot| {
                    snapshot
                        .favorites
                        .iter()
                        .map(|favorite| {
                            favorite
                                .to_colour()
                                .with_brightness(Brightness::MAX)
                                .to_rgb()
                        })
                        .collect()
                })
                .unwrap_or_default(),
            setup,
            can_configure,
        }
    }

    pub fn set_power(&mut self, on: bool) -> Option<PanelMessage> {
        self.send(BulbCommand::SetPower(if on {
            Power::On
        } else {
            Power::Off
        }))
    }

    /// Clique ou arraste no disco de cores (mantém o brilho atual).
    pub fn pick_colour(&mut self, point: DiscPoint) -> Option<PanelMessage> {
        let colour = colour_at(point);
        self.send(BulbCommand::SetColour(
            colour.with_brightness(self.current_brightness()),
        ))
    }

    pub fn set_brightness(&mut self, percent: u8) -> Option<PanelMessage> {
        let brightness = Brightness::from_percent(percent.clamp(1, 100)).expect("1 a 100");
        self.send(BulbCommand::SetBrightness(brightness))
    }

    pub fn set_temperature(&mut self, percent: u8) -> Option<PanelMessage> {
        let temperature = ColorTemperature::from_percent(percent.min(100)).expect("0 a 100");
        self.send(BulbCommand::SetTemperature(temperature))
    }

    /// Troca de aba: muda a lâmpada para o modo da aba, com a última cor/temperatura usada.
    pub fn select_tab(&mut self, tab: Tab) -> Option<PanelMessage> {
        if tab == self.tab {
            return None;
        }
        match tab {
            Tab::Colour => {
                let colour = self.last_colour.with_value(self.current_brightness());
                self.send(BulbCommand::SetColour(colour))
            }
            Tab::White => self.send(BulbCommand::SetTemperature(self.last_temperature)),
        }
    }

    pub fn apply_favorite(&mut self, slot: u8) -> Option<PanelMessage> {
        self.is_online()
            .then_some(PanelMessage::ApplyFavorite { slot })
    }

    pub fn save_favorite(&mut self, slot: u8) -> Option<PanelMessage> {
        self.is_online()
            .then_some(PanelMessage::SaveFavorite { slot })
    }

    fn is_online(&self) -> bool {
        self.state.is_some()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.connection == ConnectionDto::Online)
    }

    /// Aplica o comando na tela (otimista) e devolve a mensagem para o daemon.
    fn send(&mut self, command: BulbCommand) -> Option<PanelMessage> {
        if !self.is_online() {
            return None;
        }
        self.state = self.state.map(|state| state.apply(&command));
        self.remember_mode();
        Some(PanelMessage::Command {
            command: CommandDto::from(&command),
        })
    }

    /// Acompanha o modo do estado: aba, última cor e última temperatura.
    fn remember_mode(&mut self) {
        match self.state.map(|state| state.mode) {
            Some(LightMode::Colour(hsv)) => {
                self.tab = Tab::Colour;
                self.last_colour = hsv;
            }
            Some(LightMode::White { temperature, .. }) => {
                self.tab = Tab::White;
                self.last_temperature = temperature;
            }
            Some(LightMode::Scene | LightMode::Music) | None => {}
        }
    }

    fn current_brightness(&self) -> Brightness {
        self.state
            .and_then(|state| state.mode.brightness())
            .unwrap_or(self.last_colour.value())
    }

    /// Passos da configuração inicial (não dependem da lâmpada estar online).
    pub fn setup(&self, request: SetupRequestDto) -> PanelMessage {
        PanelMessage::Setup { request }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgin_ipc::messages::{FavoriteDto, ModeDto};

    fn white_dto(on: bool, brightness: u16, temperature: u16) -> StateDto {
        StateDto {
            on,
            mode: ModeDto::White {
                brightness,
                temperature,
            },
        }
    }

    fn colour_dto(hue: u16, saturation: u16, value: u16) -> StateDto {
        StateDto {
            on: true,
            mode: ModeDto::Colour {
                hue,
                saturation,
                value,
            },
        }
    }

    fn snapshot(connection: ConnectionDto, state: Option<StateDto>) -> SnapshotDto {
        SnapshotDto {
            bulb_name: Some("Quarto".to_owned()),
            connection,
            state,
            favorites: vec![
                FavoriteDto {
                    hue: 0,
                    saturation: 1000,
                },
                FavoriteDto {
                    hue: 240,
                    saturation: 1000,
                },
            ],
            setup: None,
        }
    }

    fn online(state: StateDto) -> PanelViewModel {
        let mut model = PanelViewModel::new();
        model.on_snapshot(snapshot(ConnectionDto::Online, Some(state)));
        model
    }

    fn command(message: Option<PanelMessage>) -> CommandDto {
        match message {
            Some(PanelMessage::Command { command }) => command,
            other => panic!("esperava um comando, veio {other:?}"),
        }
    }

    #[test]
    fn before_the_first_snapshot_everything_is_disabled() {
        let view = PanelViewModel::new().view();

        assert!(!view.controls_enabled);
        assert_eq!(view.notice.as_deref(), Some("Conectando ao app…"));
    }

    #[test]
    fn shows_a_white_bulb() {
        let view = online(white_dto(true, 650, 300)).view();

        assert_eq!(view.bulb_name, "Quarto");
        assert!(view.controls_enabled && view.power_on);
        assert_eq!(view.tab, Tab::White);
        assert_eq!(view.brightness_percent, 65);
        assert_eq!(view.temperature_percent, 30);
        assert_eq!(
            view.full_colour,
            ColorTemperature::try_from_permille(300)
                .unwrap()
                .display_colour()
        );
        assert_eq!(view.notice, None);
        assert_eq!(view.favorites, [Rgb::new(255, 0, 0), Rgb::new(0, 0, 255)]);
    }

    #[test]
    fn shows_a_colour_bulb_with_the_marker_on_the_disc() {
        let view = online(colour_dto(90, 1000, 400)).view();

        assert_eq!(view.tab, Tab::Colour);
        assert_eq!(view.brightness_percent, 40);
        assert_eq!(view.full_colour, Rgb::new(128, 255, 0));
        assert!(
            (view.marker.x - 1.0).abs() < 0.01 && view.marker.y.abs() < 0.01,
            "{:?}",
            view.marker
        );
    }

    #[test]
    fn an_offline_bulb_shows_a_notice_and_disables_the_controls() {
        let mut model = PanelViewModel::new();
        model.on_snapshot(snapshot(
            ConnectionDto::Offline,
            Some(white_dto(true, 500, 0)),
        ));

        let view = model.view();
        assert!(!view.controls_enabled);
        assert_eq!(
            view.notice.as_deref(),
            Some("A lâmpada não está respondendo na rede.")
        );
        assert_eq!(model.set_power(false), None, "nada é enviado sem conexão");
    }

    #[test]
    fn notices_for_each_problem() {
        let notice = |connection| {
            let mut model = PanelViewModel::new();
            model.on_snapshot(snapshot(connection, None));
            model.view().notice
        };

        assert_eq!(
            notice(ConnectionDto::NotConfigured).as_deref(),
            Some("Nenhuma lâmpada configurada.")
        );
        assert_eq!(
            notice(ConnectionDto::Connecting).as_deref(),
            Some("Conectando à lâmpada…")
        );
        assert_eq!(
            notice(ConnectionDto::KeyRejected).as_deref(),
            Some("A lâmpada recusou a chave. Tentando recuperar…")
        );
        assert_eq!(
            notice(ConnectionDto::NeedsReconfiguration).as_deref(),
            Some("É preciso configurar a lâmpada de novo.")
        );
    }

    #[test]
    fn the_power_switch_sends_and_updates_at_once() {
        let mut model = online(white_dto(true, 500, 0));

        assert_eq!(
            command(model.set_power(false)),
            CommandDto::SetPower { on: false }
        );
        assert!(!model.view().power_on);
    }

    #[test]
    fn picking_on_the_disc_keeps_the_brightness() {
        let mut model = online(colour_dto(0, 1000, 400));

        let sent = command(model.pick_colour(DiscPoint { x: 0.0, y: 1.0 }));

        assert_eq!(
            sent,
            CommandDto::SetColour {
                hue: 180,
                saturation: 1000,
                value: 400
            }
        );
        assert_eq!(model.view().full_colour, Rgb::new(0, 255, 255));
    }

    #[test]
    fn picking_on_the_disc_from_white_mode_switches_to_colour() {
        let mut model = online(white_dto(true, 300, 0));

        let sent = command(model.pick_colour(DiscPoint { x: 0.0, y: -1.0 }));

        assert_eq!(
            sent,
            CommandDto::SetColour {
                hue: 0,
                saturation: 1000,
                value: 300
            }
        );
        assert_eq!(model.view().tab, Tab::Colour);
    }

    #[test]
    fn the_brightness_slider_goes_from_1_to_100() {
        let mut model = online(white_dto(true, 500, 0));

        assert_eq!(
            command(model.set_brightness(0)),
            CommandDto::SetBrightness { brightness: 10 }
        );
        assert_eq!(
            command(model.set_brightness(73)),
            CommandDto::SetBrightness { brightness: 730 }
        );
        assert_eq!(model.view().brightness_percent, 73);
    }

    #[test]
    fn the_temperature_slider_sets_white() {
        let mut model = online(white_dto(true, 500, 0));

        assert_eq!(
            command(model.set_temperature(80)),
            CommandDto::SetTemperature { temperature: 800 }
        );
        assert_eq!(model.view().temperature_percent, 80);
    }

    #[test]
    fn switching_tabs_returns_to_the_last_colour_or_temperature() {
        let mut model = online(colour_dto(240, 800, 600));
        model.on_snapshot(snapshot(
            ConnectionDto::Online,
            Some(white_dto(true, 600, 700)),
        ));

        let to_colour = command(model.select_tab(Tab::Colour));
        assert_eq!(
            to_colour,
            CommandDto::SetColour {
                hue: 240,
                saturation: 800,
                value: 600
            }
        );

        let to_white = command(model.select_tab(Tab::White));
        assert_eq!(to_white, CommandDto::SetTemperature { temperature: 700 });
        assert_eq!(model.select_tab(Tab::White), None, "já está na aba");
    }

    #[test]
    fn favorites_are_applied_and_saved_by_slot() {
        let mut model = online(colour_dto(0, 1000, 500));

        assert_eq!(
            model.apply_favorite(1),
            Some(PanelMessage::ApplyFavorite { slot: 1 })
        );
        assert_eq!(
            model.save_favorite(3),
            Some(PanelMessage::SaveFavorite { slot: 3 })
        );
    }

    #[test]
    fn the_setup_is_shown_and_its_steps_are_sent() {
        let mut model = PanelViewModel::new();
        let mut with_setup = snapshot(ConnectionDto::NotConfigured, None);
        with_setup.setup = Some(SetupStateDto::AwaitingUserCode);
        model.on_snapshot(with_setup);

        assert_eq!(model.view().setup, Some(SetupStateDto::AwaitingUserCode));
        assert_eq!(
            model.setup(SetupRequestDto::SubmitUserCode {
                user_code: "Abc1234".to_owned()
            }),
            PanelMessage::Setup {
                request: SetupRequestDto::SubmitUserCode {
                    user_code: "Abc1234".to_owned()
                }
            }
        );
    }

    #[test]
    fn offers_configuration_only_when_needed_and_not_already_open() {
        let can_configure = |connection, setup: Option<SetupStateDto>| {
            let mut model = PanelViewModel::new();
            let mut snapshot = snapshot(connection, None);
            snapshot.setup = setup;
            model.on_snapshot(snapshot);
            model.view().can_configure
        };

        assert!(can_configure(ConnectionDto::NotConfigured, None));
        assert!(can_configure(ConnectionDto::NeedsReconfiguration, None));
        assert!(!can_configure(ConnectionDto::Online, None));
        assert!(!can_configure(ConnectionDto::Offline, None));
        assert!(!can_configure(
            ConnectionDto::NotConfigured,
            Some(SetupStateDto::AwaitingUserCode)
        ));
        assert!(!PanelViewModel::new().view().can_configure);
    }

    #[test]
    fn a_new_snapshot_replaces_the_optimistic_state() {
        let mut model = online(white_dto(true, 500, 0));
        model.set_brightness(90);

        model.on_snapshot(snapshot(
            ConnectionDto::Online,
            Some(white_dto(true, 200, 0)),
        ));

        assert_eq!(model.view().brightness_percent, 20);
    }
}
