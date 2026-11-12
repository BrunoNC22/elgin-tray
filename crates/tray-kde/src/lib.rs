//! Adaptador: bandeja do KDE Plasma (StatusNotifierItem).
//!
//! Só compila no Linux; nos outros sistemas a crate fica vazia.
//!
//! O `ksni` publica o ícone no DBus a partir de tarefas do tokio do próprio daemon. O daemon manda o
//! último [`TrayViewModel`] com [`KdeTray::show`] (sem esperar: vai por um `watch` e uma tarefa
//! repassa ao `ksni`); cliques, rolagem e itens do menu voltam pelo callback `on_event`.

#![cfg(target_os = "linux")]

mod menu;
mod pixmap;
mod scroll;

use std::sync::Arc;

use elgin_application::tray::{IconSpec, Indicator, TrayAction, TrayViewModel};
use tokio::sync::watch;

use crate::scroll::ScrollSteps;

/// Onde o usuário clicou, em coordenadas globais da tela (o Plasma manda junto com o clique).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickPoint {
    pub x: i32,
    pub y: i32,
}

/// Recebe cada interação com o ícone.
type EventHandler = Arc<dyn Fn(TrayAction, Option<ClickPoint>) + Send + Sync>;

/// O ícone visto pelo daemon: guarda o último estado e o `ksni` mostra.
#[derive(Debug)]
pub struct KdeTray {
    views: watch::Sender<Option<TrayViewModel>>,
}

impl KdeTray {
    /// Mostra `view` no ícone (o `ksni` avisa o Plasma do que mudou).
    pub fn show(&self, view: &TrayViewModel) {
        self.views.send_replace(Some(view.clone()));
    }
}

/// Registra o ícone na bandeja. Se o Plasma ainda não subiu (início da sessão), o ícone aparece
/// quando ele subir. Precisa rodar dentro de um runtime do tokio.
pub async fn start<E>(on_event: E) -> Result<KdeTray, String>
where
    E: Fn(TrayAction, Option<ClickPoint>) + Send + Sync + 'static,
{
    use ksni::TrayMethods;

    let handle = SniTray::new(Arc::new(on_event))
        .assume_sni_available(true)
        .spawn()
        .await
        .map_err(|error| format!("não foi possível criar o ícone da bandeja: {error}"))?;
    let (views, mut latest) = watch::channel(None::<TrayViewModel>);
    tokio::spawn(async move {
        while latest.changed().await.is_ok() {
            let Some(view) = latest.borrow_and_update().clone() else {
                continue;
            };
            if handle.update(|tray| tray.set_view(view)).await.is_none() {
                break;
            }
        }
    });
    Ok(KdeTray { views })
}

/// O item da bandeja (StatusNotifierItem) que o `ksni` publica.
pub(crate) struct SniTray {
    view: Option<TrayViewModel>,
    /// O ícone já convertido para os tamanhos da bandeja (só muda quando o estado do ícone muda).
    icons: Vec<ksni::Icon>,
    scroll: ScrollSteps,
    on_event: EventHandler,
}

impl SniTray {
    fn new(on_event: EventHandler) -> Self {
        Self {
            view: None,
            icons: pixmap::pixmaps(&IconSpec::Unavailable(Indicator::Warning)),
            scroll: ScrollSteps::default(),
            on_event,
        }
    }

    fn set_view(&mut self, view: TrayViewModel) {
        if self.view.as_ref().map(|current| current.icon) != Some(view.icon) {
            self.icons = pixmap::pixmaps(&view.icon);
        }
        self.view = Some(view);
    }

    fn emit(&self, action: TrayAction, point: Option<ClickPoint>) {
        (self.on_event)(action, point);
    }
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "elgin-tray".to_owned()
    }

    fn title(&self) -> String {
        "elgin-tray".to_owned()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self
                .view
                .as_ref()
                .map_or_else(|| "elgin-tray".to_owned(), |view| view.tooltip.clone()),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        self.view
            .as_ref()
            .map(|view| menu::menu(&view.menu))
            .unwrap_or_default()
    }

    /// Clique esquerdo: abre/fecha o painel.
    fn activate(&mut self, x: i32, y: i32) {
        self.emit(TrayAction::Click, Some(ClickPoint { x, y }));
    }

    /// Clique do meio: liga/desliga.
    fn secondary_activate(&mut self, x: i32, y: i32) {
        self.emit(TrayAction::MiddleClick, Some(ClickPoint { x, y }));
    }

    /// Rolagem vertical sobre o ícone: brilho.
    fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
        if orientation != ksni::Orientation::Vertical {
            return;
        }
        let steps = self.scroll.add(delta);
        if steps != 0 {
            self.emit(TrayAction::Scroll(steps), None);
        }
    }

    fn watcher_online(&self) {
        log::info!("ícone registrado na bandeja do Plasma");
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        log::warn!("bandeja do Plasma indisponível ({reason:?}); o ícone volta quando ela voltar");
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use elgin_application::tray::{MenuAction, MenuItem};
    use elgin_domain::Rgb;
    use ksni::Tray;
    use std::sync::Mutex;

    type Event = (TrayAction, Option<ClickPoint>);

    /// Guarda as interações que o ícone mandou (compartilhado entre clones).
    #[derive(Debug, Clone, Default)]
    pub(crate) struct SpyEvents(Arc<Mutex<Vec<Event>>>);

    impl SpyEvents {
        pub(crate) fn taken(&self) -> Vec<Event> {
            std::mem::take(&mut self.0.lock().unwrap())
        }
    }

    pub(crate) fn tray(events: &SpyEvents) -> SniTray {
        let events = events.clone();
        SniTray::new(Arc::new(move |action, point| {
            events.0.lock().unwrap().push((action, point));
        }))
    }

    fn view(icon: IconSpec) -> TrayViewModel {
        TrayViewModel {
            icon,
            tooltip: "Quarto — ligada, 40%".to_owned(),
            menu: vec![MenuItem::Action {
                action: MenuAction::Quit,
                label: "Sair".to_owned(),
                enabled: true,
            }],
        }
    }

    #[test]
    fn a_left_click_opens_the_panel_where_it_was_clicked() {
        let events = SpyEvents::default();
        let mut tray = tray(&events);

        tray.activate(1380, 20);

        assert_eq!(
            events.taken(),
            [(TrayAction::Click, Some(ClickPoint { x: 1380, y: 20 }))]
        );
    }

    #[test]
    fn a_middle_click_toggles_the_power() {
        let events = SpyEvents::default();
        let mut tray = tray(&events);

        tray.secondary_activate(1380, 20);

        assert_eq!(
            events.taken(),
            [(TrayAction::MiddleClick, Some(ClickPoint { x: 1380, y: 20 }))]
        );
    }

    #[test]
    fn vertical_scroll_becomes_brightness_steps() {
        let events = SpyEvents::default();
        let mut tray = tray(&events);

        tray.scroll(240, ksni::Orientation::Vertical);
        tray.scroll(60, ksni::Orientation::Vertical);
        tray.scroll(-120, ksni::Orientation::Vertical);

        assert_eq!(
            events.taken(),
            [
                (TrayAction::Scroll(2), None),
                (TrayAction::Scroll(-1), None)
            ]
        );
    }

    #[test]
    fn horizontal_scroll_is_ignored() {
        let events = SpyEvents::default();
        let mut tray = tray(&events);

        tray.scroll(120, ksni::Orientation::Horizontal);

        assert!(events.taken().is_empty());
    }

    #[test]
    fn before_the_first_state_it_shows_the_warning_icon_and_no_menu() {
        let tray = tray(&SpyEvents::default());

        assert_eq!(
            tray.icon_pixmap()[0].data,
            pixmap::pixmaps(&IconSpec::Unavailable(Indicator::Warning))[0].data
        );
        assert_eq!(tray.tool_tip().title, "elgin-tray");
        assert!(tray.menu().is_empty());
    }

    #[test]
    fn a_new_state_changes_icon_tooltip_and_menu() {
        let mut tray = tray(&SpyEvents::default());
        let on = IconSpec::On(Rgb::new(255, 0, 0));

        tray.set_view(view(on));

        assert_eq!(tray.icon_pixmap()[0].data, pixmap::pixmaps(&on)[0].data);
        assert_eq!(tray.tool_tip().title, "Quarto — ligada, 40%");
        assert_eq!(tray.menu().len(), 1);
    }
}
