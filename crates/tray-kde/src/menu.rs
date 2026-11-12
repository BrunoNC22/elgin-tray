//! O menu do clique direito no formato do DBusMenu.

use elgin_application::tray::{MenuItem, TrayAction};
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};

use crate::SniTray;

/// Converte o menu montado pela aplicação; cada item ativado vira um [`TrayAction::Menu`].
pub(crate) fn menu(items: &[MenuItem]) -> Vec<ksni::MenuItem<SniTray>> {
    items.iter().map(item).collect()
}

fn item(item: &MenuItem) -> ksni::MenuItem<SniTray> {
    match item {
        MenuItem::Action {
            action,
            label: text,
            enabled,
        } => {
            let action = *action;
            ksni::MenuItem::Standard(StandardItem {
                label: label(text),
                enabled: *enabled,
                activate: Box::new(move |tray: &mut SniTray| {
                    tray.emit(TrayAction::Menu(action), None)
                }),
                ..Default::default()
            })
        }
        MenuItem::Check {
            action,
            label: text,
            checked,
            enabled,
        } => {
            let action = *action;
            ksni::MenuItem::Checkmark(CheckmarkItem {
                label: label(text),
                checked: *checked,
                enabled: *enabled,
                activate: Box::new(move |tray: &mut SniTray| {
                    tray.emit(TrayAction::Menu(action), None)
                }),
                ..Default::default()
            })
        }
        MenuItem::Submenu {
            label: text,
            enabled,
            items,
        } => ksni::MenuItem::SubMenu(SubMenu {
            label: label(text),
            enabled: *enabled,
            submenu: menu(items),
            ..Default::default()
        }),
        MenuItem::Separator => ksni::MenuItem::Separator,
    }
}

/// No DBusMenu, `_` marca a tecla de atalho do item: dobrado, aparece como texto.
fn label(text: &str) -> String {
    text.replace('_', "__")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{SpyEvents, tray};
    use elgin_application::tray::MenuAction;
    use elgin_domain::FavoriteSlot;

    fn sample() -> Vec<MenuItem> {
        vec![
            MenuItem::Check {
                action: MenuAction::TogglePower,
                label: "Ligada".to_owned(),
                checked: true,
                enabled: true,
            },
            MenuItem::Submenu {
                label: "Cores favoritas".to_owned(),
                enabled: false,
                items: vec![MenuItem::Action {
                    action: MenuAction::ApplyFavorite(FavoriteSlot::try_new(0).unwrap()),
                    label: "Vermelho".to_owned(),
                    enabled: false,
                }],
            },
            MenuItem::Separator,
            MenuItem::Action {
                action: MenuAction::Quit,
                label: "Sair_agora".to_owned(),
                enabled: true,
            },
        ]
    }

    #[test]
    fn items_keep_label_state_and_structure() {
        let converted = menu(&sample());

        let ksni::MenuItem::Checkmark(power) = &converted[0] else {
            panic!("{:?}", converted[0])
        };
        assert_eq!(
            (power.label.as_str(), power.checked, power.enabled),
            ("Ligada", true, true)
        );
        let ksni::MenuItem::SubMenu(favourites) = &converted[1] else {
            panic!("{:?}", converted[1])
        };
        assert_eq!(favourites.label, "Cores favoritas");
        assert!(!favourites.enabled);
        let ksni::MenuItem::Standard(red) = &favourites.submenu[0] else {
            panic!("{:?}", favourites.submenu[0])
        };
        assert_eq!((red.label.as_str(), red.enabled), ("Vermelho", false));
        assert!(matches!(converted[2], ksni::MenuItem::Separator));
        let ksni::MenuItem::Standard(quit) = &converted[3] else {
            panic!("{:?}", converted[3])
        };
        assert_eq!(quit.label, "Sair__agora", "sublinhado não vira atalho");
    }

    #[test]
    fn activating_an_item_sends_its_action() {
        let events = SpyEvents::default();
        let mut tray = tray(&events);
        let converted = menu(&sample());

        let ksni::MenuItem::Checkmark(power) = &converted[0] else {
            unreachable!()
        };
        (power.activate)(&mut tray);
        let ksni::MenuItem::SubMenu(favourites) = &converted[1] else {
            unreachable!()
        };
        let ksni::MenuItem::Standard(red) = &favourites.submenu[0] else {
            unreachable!()
        };
        (red.activate)(&mut tray);

        assert_eq!(
            events.taken(),
            [
                (TrayAction::Menu(MenuAction::TogglePower), None),
                (
                    TrayAction::Menu(MenuAction::ApplyFavorite(FavoriteSlot::try_new(0).unwrap())),
                    None
                ),
            ]
        );
    }
}
