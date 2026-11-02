//! O que a bandeja mostra (ícone, dica e menu) e o que as ações dela fazem.
//!
//! As diferenças entre sistemas entram como dados (ex.: [`UpdatePolicy`]), nunca como `cfg`.

use elgin_domain::{BulbCommand, FavoriteColour, FavoriteSlot, Favorites, LightMode, Rgb};

use crate::status::{BulbSnapshot, ConnectionStatus};

/// Como o ícone da bandeja deve ser desenhado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconSpec {
    /// Ligada: lâmpada preenchida com a cor que ela está mostrando.
    On(Rgb),
    /// Desligada: só o contorno.
    Off,
    /// Sem controle da lâmpada: cinza com um ponto indicador.
    Unavailable(Indicator),
}

/// O ponto no canto do ícone quando a lâmpada não está disponível.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Indicator {
    /// Conectando, offline ou não configurada (laranja).
    Warning,
    /// Chave recusada ou precisa reconfigurar (vermelho).
    Error,
}

/// Como as atualizações chegam neste sistema (decidido pelo composition root).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePolicy {
    /// O próprio app baixa e instala (Windows).
    AutoInstall,
    /// O gerenciador de pacotes instala; o app só avisa (Arch/AUR).
    NotifyOnly,
}

/// Tudo de que a bandeja depende.
#[derive(Debug, Clone, Copy)]
pub struct TrayInputs<'a> {
    pub bulb_name: Option<&'a str>,
    pub snapshot: &'a BulbSnapshot,
    pub favorites: &'a Favorites,
    pub autostart: bool,
    pub update_policy: UpdatePolicy,
    /// Versão nova disponível (já detectada), se houver.
    pub available_update: Option<&'a str>,
    pub app_version: &'a str,
}

/// O que a bandeja deve mostrar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayViewModel {
    pub icon: IconSpec,
    pub tooltip: String,
    pub menu: Vec<MenuItem>,
}

/// Item do menu do clique direito.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Action {
        action: MenuAction,
        label: String,
        enabled: bool,
    },
    Check {
        action: MenuAction,
        label: String,
        checked: bool,
        enabled: bool,
    },
    Submenu {
        label: String,
        enabled: bool,
        items: Vec<MenuItem>,
    },
    Separator,
}

/// O que um item do menu faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAction {
    TogglePower,
    ApplyFavorite(FavoriteSlot),
    OpenPanel,
    Configure,
    ToggleAutostart,
    CheckUpdates,
    ShowUpdate,
    Quit,
}

impl MenuAction {
    /// Identificador estável do item (as bandejas dos sistemas usam texto).
    pub fn id(self) -> String {
        match self {
            Self::TogglePower => "toggle_power".to_owned(),
            Self::ApplyFavorite(slot) => format!("favorite:{}", slot.index()),
            Self::OpenPanel => "open_panel".to_owned(),
            Self::Configure => "configure".to_owned(),
            Self::ToggleAutostart => "autostart".to_owned(),
            Self::CheckUpdates => "check_updates".to_owned(),
            Self::ShowUpdate => "show_update".to_owned(),
            Self::Quit => "quit".to_owned(),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "toggle_power" => Self::TogglePower,
            "open_panel" => Self::OpenPanel,
            "configure" => Self::Configure,
            "autostart" => Self::ToggleAutostart,
            "check_updates" => Self::CheckUpdates,
            "show_update" => Self::ShowUpdate,
            "quit" => Self::Quit,
            other => {
                let index = other.strip_prefix("favorite:")?.parse().ok()?;
                Self::ApplyFavorite(FavoriteSlot::try_new(index).ok()?)
            }
        })
    }
}

/// Interação com o ícone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Clique esquerdo (abre/fecha o painel).
    Click,
    /// Clique do meio (liga/desliga).
    MiddleClick,
    /// Rolagem do mouse sobre o ícone: passos positivos para cima.
    Scroll(i32),
    Menu(MenuAction),
}

/// Pontos percentuais de brilho por passo de rolagem.
pub const SCROLL_STEP_PERCENT: i32 = 5;

/// Monta o que a bandeja mostra.
pub fn present(inputs: &TrayInputs<'_>) -> TrayViewModel {
    let snapshot = inputs.snapshot;
    let online = snapshot.connection == ConnectionStatus::Online;
    let state = snapshot.state.filter(|_| online);
    let name = inputs.bulb_name.unwrap_or("elgin-tray");

    let icon = match (snapshot.connection, state) {
        (ConnectionStatus::Online, Some(state)) if state.power.is_on() => {
            // Cena e música não têm uma cor fixa: lâmpada branca.
            IconSpec::On(
                state
                    .mode
                    .display_colour()
                    .unwrap_or(Rgb::new(255, 255, 255)),
            )
        }
        (ConnectionStatus::Online, Some(_)) => IconSpec::Off,
        (ConnectionStatus::KeyRejected | ConnectionStatus::NeedsReconfiguration, _) => {
            IconSpec::Unavailable(Indicator::Error)
        }
        _ => IconSpec::Unavailable(Indicator::Warning),
    };

    let status = match (snapshot.connection, state) {
        (ConnectionStatus::Online, Some(state)) if state.power.is_on() => {
            match state.mode.brightness() {
                Some(brightness) => format!("ligada, {}%", brightness.percent()),
                None if state.mode == LightMode::Music => "ligada (música)".to_owned(),
                None => "ligada (cena)".to_owned(),
            }
        }
        (ConnectionStatus::Online, _) => "desligada".to_owned(),
        (ConnectionStatus::NotConfigured, _) => "nenhuma lâmpada configurada".to_owned(),
        (ConnectionStatus::Connecting, _) => "conectando…".to_owned(),
        (ConnectionStatus::Offline, _) => "sem conexão".to_owned(),
        (ConnectionStatus::KeyRejected, _) => "a lâmpada recusou a chave".to_owned(),
        (ConnectionStatus::NeedsReconfiguration, _) => {
            "é preciso reconfigurar a lâmpada".to_owned()
        }
    };

    TrayViewModel {
        icon,
        tooltip: format!("{name} — {status}"),
        // "Ligada" mostra o último estado conhecido mesmo sem conexão (desabilitado).
        menu: menu(
            inputs,
            state.is_some(),
            snapshot.state.is_some_and(|state| state.power.is_on()),
        ),
    }
}

fn menu(inputs: &TrayInputs<'_>, available: bool, on: bool) -> Vec<MenuItem> {
    let favourites = FavoriteSlot::all()
        .map(|slot| MenuItem::Action {
            action: MenuAction::ApplyFavorite(slot),
            label: colour_name(inputs.favorites.get(slot)).to_owned(),
            enabled: available,
        })
        .collect();
    let configure = if inputs.snapshot.connection == ConnectionStatus::NotConfigured {
        "Configurar lâmpada…"
    } else {
        "Reconfigurar lâmpada…"
    };
    let action = |action, label: &str| MenuItem::Action {
        action,
        label: label.to_owned(),
        enabled: true,
    };

    let mut menu = vec![
        MenuItem::Check {
            action: MenuAction::TogglePower,
            label: "Ligada".to_owned(),
            checked: on,
            enabled: available,
        },
        MenuItem::Submenu {
            label: "Cores favoritas".to_owned(),
            enabled: available,
            items: favourites,
        },
        MenuItem::Separator,
        action(MenuAction::OpenPanel, "Abrir painel"),
        action(MenuAction::Configure, configure),
        MenuItem::Check {
            action: MenuAction::ToggleAutostart,
            label: "Iniciar com o sistema".to_owned(),
            checked: inputs.autostart,
            enabled: true,
        },
    ];
    match (inputs.update_policy, inputs.available_update) {
        (UpdatePolicy::AutoInstall, None) => {
            menu.push(action(MenuAction::CheckUpdates, "Verificar atualizações"))
        }
        (UpdatePolicy::AutoInstall, Some(version)) => {
            menu.push(action(
                MenuAction::CheckUpdates,
                &format!("Instalar versão {version}"),
            ));
        }
        (UpdatePolicy::NotifyOnly, Some(version)) => menu.push(action(
            MenuAction::ShowUpdate,
            &format!("Nova versão {version} — atualize pelo yay"),
        )),
        (UpdatePolicy::NotifyOnly, None) => {}
    }
    menu.push(MenuItem::Separator);
    menu.push(MenuItem::Action {
        action: MenuAction::ShowUpdate,
        label: format!("elgin-tray {}", inputs.app_version),
        enabled: false,
    });
    menu.push(action(MenuAction::Quit, "Sair"));
    menu
}

/// O comando para a lâmpada que uma ação da bandeja pede (`None` se a ação não mexe na lâmpada ou
/// se a lâmpada não está disponível).
pub fn command_for(
    action: TrayAction,
    snapshot: &BulbSnapshot,
    favorites: &Favorites,
) -> Option<BulbCommand> {
    let state = snapshot
        .state
        .filter(|_| snapshot.connection == ConnectionStatus::Online)?;
    match action {
        TrayAction::MiddleClick | TrayAction::Menu(MenuAction::TogglePower) => {
            Some(BulbCommand::TogglePower)
        }
        TrayAction::Scroll(steps) => {
            let percent = steps.saturating_mul(SCROLL_STEP_PERCENT).clamp(-100, 100);
            Some(BulbCommand::StepBrightness(percent as i8))
        }
        TrayAction::Menu(MenuAction::ApplyFavorite(slot)) => {
            Some(favorites.apply_command(slot, &state))
        }
        TrayAction::Click | TrayAction::Menu(_) => None,
    }
}

/// Nome aproximado de uma cor favorita, para o menu.
pub fn colour_name(colour: FavoriteColour) -> &'static str {
    if colour.saturation().percent() < 25 {
        return "Branco";
    }
    match colour.hue().degrees() {
        0..=14 | 345..=359 => "Vermelho",
        15..=44 => "Laranja",
        45..=74 => "Amarelo",
        75..=164 => "Verde",
        165..=199 => "Ciano",
        200..=259 => "Azul",
        260..=299 => "Roxo",
        _ => "Rosa",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgin_domain::{Brightness, BulbState, ColorTemperature, Hsv, Hue, Power, Saturation};

    fn white(percent: u8) -> LightMode {
        LightMode::White {
            brightness: Brightness::from_percent(percent).unwrap(),
            temperature: ColorTemperature::COOLEST,
        }
    }

    fn snapshot(connection: ConnectionStatus, power: Power, mode: LightMode) -> BulbSnapshot {
        BulbSnapshot {
            connection,
            state: Some(BulbState { power, mode }),
        }
    }

    fn online(power: Power, mode: LightMode) -> BulbSnapshot {
        snapshot(ConnectionStatus::Online, power, mode)
    }

    fn view(snapshot: &BulbSnapshot) -> TrayViewModel {
        view_with(snapshot, UpdatePolicy::AutoInstall, None)
    }

    fn view_with(
        snapshot: &BulbSnapshot,
        policy: UpdatePolicy,
        update: Option<&str>,
    ) -> TrayViewModel {
        present(&TrayInputs {
            bulb_name: Some("Quarto"),
            snapshot,
            favorites: &Favorites::default(),
            autostart: true,
            update_policy: policy,
            available_update: update,
            app_version: "1.2.3",
        })
    }

    fn labels(menu: &[MenuItem]) -> Vec<String> {
        menu.iter()
            .map(|item| match item {
                MenuItem::Action { label, .. }
                | MenuItem::Check { label, .. }
                | MenuItem::Submenu { label, .. } => label.clone(),
                MenuItem::Separator => "---".to_owned(),
            })
            .collect()
    }

    fn red(value: u8) -> LightMode {
        LightMode::Colour(Hsv::new(
            Hue::new(0),
            Saturation::FULL,
            Brightness::from_percent(value).unwrap(),
        ))
    }

    #[test]
    fn an_on_bulb_shows_its_full_colour_and_brightness() {
        let model = view(&online(Power::On, red(30)));

        assert_eq!(model.icon, IconSpec::On(Rgb::new(255, 0, 0)));
        assert_eq!(model.tooltip, "Quarto — ligada, 30%");
    }

    #[test]
    fn an_off_bulb_is_outlined() {
        let model = view(&online(Power::Off, white(80)));

        assert_eq!(model.icon, IconSpec::Off);
        assert_eq!(model.tooltip, "Quarto — desligada");
    }

    #[test]
    fn a_scene_is_shown_as_a_white_bulb() {
        let model = view(&online(Power::On, LightMode::Scene));

        assert_eq!(model.icon, IconSpec::On(Rgb::new(255, 255, 255)));
        assert_eq!(model.tooltip, "Quarto — ligada (cena)");
    }

    #[test]
    fn unavailable_statuses_use_the_indicators() {
        let cases = [
            (
                ConnectionStatus::Connecting,
                Indicator::Warning,
                "Quarto — conectando…",
            ),
            (
                ConnectionStatus::Offline,
                Indicator::Warning,
                "Quarto — sem conexão",
            ),
            (
                ConnectionStatus::KeyRejected,
                Indicator::Error,
                "Quarto — a lâmpada recusou a chave",
            ),
            (
                ConnectionStatus::NeedsReconfiguration,
                Indicator::Error,
                "Quarto — é preciso reconfigurar a lâmpada",
            ),
        ];
        for (status, indicator, tooltip) in cases {
            let model = view(&snapshot(status, Power::On, white(50)));
            assert_eq!(model.icon, IconSpec::Unavailable(indicator), "{status:?}");
            assert_eq!(model.tooltip, tooltip);
        }
    }

    #[test]
    fn without_a_bulb_it_invites_to_configure() {
        let model = present(&TrayInputs {
            bulb_name: None,
            snapshot: &BulbSnapshot {
                connection: ConnectionStatus::NotConfigured,
                state: None,
            },
            favorites: &Favorites::default(),
            autostart: false,
            update_policy: UpdatePolicy::AutoInstall,
            available_update: None,
            app_version: "1.2.3",
        });

        assert_eq!(model.icon, IconSpec::Unavailable(Indicator::Warning));
        assert_eq!(model.tooltip, "elgin-tray — nenhuma lâmpada configurada");
        assert!(labels(&model.menu).contains(&"Configurar lâmpada…".to_owned()));
    }

    #[test]
    fn the_menu_has_every_item_in_order() {
        let model = view(&online(Power::On, white(50)));

        assert_eq!(
            labels(&model.menu),
            [
                "Ligada",
                "Cores favoritas",
                "---",
                "Abrir painel",
                "Reconfigurar lâmpada…",
                "Iniciar com o sistema",
                "Verificar atualizações",
                "---",
                "elgin-tray 1.2.3",
                "Sair",
            ]
        );
        assert!(model.menu.contains(&MenuItem::Check {
            action: MenuAction::TogglePower,
            label: "Ligada".to_owned(),
            checked: true,
            enabled: true,
        }));
        assert!(model.menu.contains(&MenuItem::Check {
            action: MenuAction::ToggleAutostart,
            label: "Iniciar com o sistema".to_owned(),
            checked: true,
            enabled: true,
        }));
    }

    #[test]
    fn favourites_are_listed_by_colour_name() {
        let model = view(&online(Power::On, white(50)));
        let MenuItem::Submenu { items, enabled, .. } = &model.menu[1] else {
            panic!("esperava o submenu")
        };

        assert!(enabled);
        assert_eq!(
            labels(items),
            ["Vermelho", "Laranja", "Amarelo", "Verde", "Azul", "Roxo"]
        );
        assert_eq!(
            items[4],
            MenuItem::Action {
                action: MenuAction::ApplyFavorite(FavoriteSlot::try_new(4).unwrap()),
                label: "Azul".to_owned(),
                enabled: true,
            }
        );
    }

    #[test]
    fn bulb_items_are_disabled_while_unavailable() {
        let model = view(&snapshot(ConnectionStatus::Offline, Power::On, white(50)));

        assert!(model.menu.contains(&MenuItem::Check {
            action: MenuAction::TogglePower,
            label: "Ligada".to_owned(),
            checked: true,
            enabled: false,
        }));
        assert!(matches!(
            &model.menu[1],
            MenuItem::Submenu { enabled: false, .. }
        ));
    }

    #[test]
    fn notify_only_shows_the_update_item_only_when_there_is_one() {
        let without = view_with(
            &online(Power::On, white(50)),
            UpdatePolicy::NotifyOnly,
            None,
        );
        let with = view_with(
            &online(Power::On, white(50)),
            UpdatePolicy::NotifyOnly,
            Some("1.3.0"),
        );

        assert!(
            !labels(&without.menu)
                .iter()
                .any(|label| label.contains("versão") || label.contains("atualiza"))
        );
        assert!(with.menu.contains(&MenuItem::Action {
            action: MenuAction::ShowUpdate,
            label: "Nova versão 1.3.0 — atualize pelo yay".to_owned(),
            enabled: true,
        }));
    }

    #[test]
    fn auto_install_shows_the_new_version_when_known() {
        let model = view_with(
            &online(Power::On, white(50)),
            UpdatePolicy::AutoInstall,
            Some("1.3.0"),
        );

        assert!(labels(&model.menu).contains(&"Instalar versão 1.3.0".to_owned()));
    }

    #[test]
    fn actions_become_bulb_commands() {
        let on = online(Power::On, white(50));
        let favorites = Favorites::default();

        assert_eq!(
            command_for(TrayAction::MiddleClick, &on, &favorites),
            Some(BulbCommand::TogglePower)
        );
        assert_eq!(
            command_for(TrayAction::Menu(MenuAction::TogglePower), &on, &favorites),
            Some(BulbCommand::TogglePower)
        );
        assert_eq!(
            command_for(TrayAction::Scroll(2), &on, &favorites),
            Some(BulbCommand::StepBrightness(10))
        );
        assert_eq!(
            command_for(TrayAction::Scroll(-1), &on, &favorites),
            Some(BulbCommand::StepBrightness(-5))
        );
        assert_eq!(
            command_for(TrayAction::Scroll(1000), &on, &favorites),
            Some(BulbCommand::StepBrightness(100))
        );
        let slot = FavoriteSlot::try_new(2).unwrap();
        assert_eq!(
            command_for(
                TrayAction::Menu(MenuAction::ApplyFavorite(slot)),
                &on,
                &favorites
            ),
            Some(favorites.apply_command(slot, &on.state.unwrap()))
        );
        assert_eq!(command_for(TrayAction::Click, &on, &favorites), None);
        assert_eq!(
            command_for(TrayAction::Menu(MenuAction::Quit), &on, &favorites),
            None
        );
    }

    #[test]
    fn nothing_is_sent_while_the_bulb_is_unavailable() {
        let offline = snapshot(ConnectionStatus::Offline, Power::On, white(50));

        assert_eq!(
            command_for(TrayAction::MiddleClick, &offline, &Favorites::default()),
            None
        );
        assert_eq!(
            command_for(TrayAction::Scroll(1), &offline, &Favorites::default()),
            None
        );
    }

    #[test]
    fn menu_action_ids_round_trip() {
        let actions = [
            MenuAction::TogglePower,
            MenuAction::ApplyFavorite(FavoriteSlot::try_new(5).unwrap()),
            MenuAction::OpenPanel,
            MenuAction::Configure,
            MenuAction::ToggleAutostart,
            MenuAction::CheckUpdates,
            MenuAction::ShowUpdate,
            MenuAction::Quit,
        ];
        for action in actions {
            assert_eq!(
                MenuAction::from_id(&action.id()),
                Some(action),
                "{}",
                action.id()
            );
        }
        assert_eq!(
            MenuAction::ApplyFavorite(FavoriteSlot::try_new(2).unwrap()).id(),
            "favorite:2"
        );
        assert_eq!(MenuAction::from_id("favorite:9"), None);
        assert_eq!(MenuAction::from_id("outro"), None);
    }

    #[test]
    fn colour_names_cover_the_hue_circle_and_white() {
        let named = |hue, saturation| {
            colour_name(FavoriteColour::new(
                Hue::new(hue),
                Saturation::try_from_permille(saturation).unwrap(),
            ))
        };

        assert_eq!(named(0, 1000), "Vermelho");
        assert_eq!(named(30, 1000), "Laranja");
        assert_eq!(named(60, 1000), "Amarelo");
        assert_eq!(named(120, 1000), "Verde");
        assert_eq!(named(180, 1000), "Ciano");
        assert_eq!(named(240, 1000), "Azul");
        assert_eq!(named(280, 1000), "Roxo");
        assert_eq!(named(320, 1000), "Rosa");
        assert_eq!(named(355, 1000), "Vermelho");
        assert_eq!(named(200, 100), "Branco");
    }
}
