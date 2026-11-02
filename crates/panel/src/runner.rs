//! O processo do painel (`elgin-tray panel`): a janela Slint na thread principal e a conexão com o
//! daemon numa thread com tokio. O view-model fica entre as duas (`Arc<Mutex>`).

use std::sync::{Arc, Mutex};

use elgin_domain::{ColorTemperature, Rgb};
use elgin_ipc::messages::{DaemonMessage, PanelMessage};
use elgin_ipc::session::PanelClient;
use elgin_ipc::transport;
use slint::{ComponentHandle, ModelRc, VecModel, Weak};
use tokio::sync::mpsc;

use crate::disc::DiscPoint;
use crate::view_model::{PanelView, PanelViewModel, Tab};
use crate::{Panel, disc_slint_image};

type SharedModel = Arc<Mutex<PanelViewModel>>;
type Outgoing = mpsc::UnboundedSender<PanelMessage>;

/// Resolução da imagem do disco (o componente tem 220 px; o dobro fica nítido em telas com escala).
const DISC_PIXELS: u32 = 440;

/// Abre o painel e só volta quando ele fecha (Esc, perda de foco ou o daemon mandar fechar).
pub fn run() -> Result<(), String> {
    let window =
        Panel::new().map_err(|error| format!("não foi possível criar o painel: {error}"))?;
    window.set_disc(disc_slint_image(DISC_PIXELS));
    window.set_warm_colour(colour(ColorTemperature::WARMEST.display_colour()));
    window.set_cool_colour(colour(ColorTemperature::COOLEST.display_colour()));

    let model: SharedModel = Arc::new(Mutex::new(PanelViewModel::new()));
    render(&window, &view(&model));
    let (outgoing, outgoing_receiver) = mpsc::unbounded_channel();
    connect_callbacks(&window, &model, &outgoing);

    let weak = window.as_weak();
    let ipc_model = Arc::clone(&model);
    std::thread::spawn(move || talk_to_daemon(&ipc_model, &weak, outgoing_receiver));

    window.show().map_err(|error| error.to_string())?;
    configure_native_window(&window);
    slint::run_event_loop().map_err(|error| error.to_string())?;
    let _ = outgoing.send(PanelMessage::Close);
    Ok(())
}

/// Cada gesto: o view-model atualiza a tela (otimista) e gera a mensagem para o daemon.
fn connect_callbacks(window: &Panel, model: &SharedModel, outgoing: &Outgoing) {
    let handler = |action: fn(&mut PanelViewModel, f32, f32) -> Option<PanelMessage>| {
        let model = Arc::clone(model);
        let outgoing = outgoing.clone();
        let weak = window.as_weak();
        move |a: f32, b: f32| {
            let message = model
                .lock()
                .ok()
                .and_then(|mut model| action(&mut model, a, b));
            if let Some(message) = message {
                let _ = outgoing.send(message);
            }
            if let Some(window) = weak.upgrade() {
                render(&window, &view(&model));
            }
        }
    };

    let power = handler(|model, on, _| model.set_power(on > 0.5));
    window.on_power_toggled(move |on| power(if on { 1.0 } else { 0.0 }, 0.0));
    let tab = handler(|model, colour, _| {
        model.select_tab(if colour > 0.5 {
            Tab::Colour
        } else {
            Tab::White
        })
    });
    window.on_tab_selected(move |colour| tab(if colour { 1.0 } else { 0.0 }, 0.0));
    let pick = handler(|model, x, y| model.pick_colour(DiscPoint { x, y }));
    window.on_colour_picked(pick);
    // Slider de brilho: 0..1 ↔ 1..100%.
    let brightness =
        handler(|model, value, _| model.set_brightness(1 + (value * 99.0).round() as u8));
    window.on_brightness_changed(move |value| brightness(value, 0.0));
    let temperature =
        handler(|model, value, _| model.set_temperature((value * 100.0).round() as u8));
    window.on_temperature_changed(move |value| temperature(value, 0.0));
    let apply = handler(|model, slot, _| model.apply_favorite(slot as u8));
    window.on_favorite_applied(move |slot| apply(slot as f32, 0.0));
    let save = handler(|model, slot, _| model.save_favorite(slot as u8));
    window.on_favorite_saved(move |slot| save(slot as f32, 0.0));
    window.on_close_requested(|| {
        let _ = slint::quit_event_loop();
    });
}

/// Fecha ao perder o foco e não aparece na barra de tarefas (Windows).
fn configure_native_window(window: &Panel) {
    use slint::winit_030::{EventResult, WinitWindowAccessor, winit};

    window.window().on_winit_window_event(|_, event| {
        if let winit::event::WindowEvent::Focused(false) = event {
            let _ = slint::quit_event_loop();
        }
        EventResult::Propagate
    });
    window.window().with_winit_window(|native| {
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowExtWindows;
            native.set_skip_taskbar(true);
        }
        native.focus_window();
    });
}

/// A conexão com o daemon: snapshots atualizam a tela; mensagens da tela vão para o daemon.
fn talk_to_daemon(
    model: &SharedModel,
    window: &Weak<Panel>,
    mut outgoing: mpsc::UnboundedReceiver<PanelMessage>,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return close_panel();
    };
    runtime.block_on(async {
        let Ok(stream) = transport::connect().await else {
            return close_panel();
        };
        let Ok(mut client) = PanelClient::start(stream).await else {
            return close_panel();
        };
        loop {
            tokio::select! {
                message = client.receive() => match message {
                    Ok(Some(DaemonMessage::Snapshot(snapshot))) => {
                        if let Ok(mut model) = model.lock() {
                            model.on_snapshot(snapshot);
                        }
                        let model = Arc::clone(model);
                        let window = window.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(window) = window.upgrade() {
                                render(&window, &view(&model));
                            }
                        });
                    }
                    // Shutdown, conexão fechada ou erro: o painel fecha.
                    _ => return close_panel(),
                },
                Some(message) = outgoing.recv() => {
                    let closing = message == PanelMessage::Close;
                    if client.send(&message).await.is_err() || closing {
                        return close_panel();
                    }
                }
            }
        }
    });
}

fn close_panel() {
    let _ = slint::invoke_from_event_loop(|| {
        let _ = slint::quit_event_loop();
    });
}

fn view(model: &SharedModel) -> PanelView {
    model
        .lock()
        .map(|model| model.view())
        .unwrap_or_else(|_| PanelViewModel::new().view())
}

/// Passa o que o view-model mostra para as propriedades da janela.
fn render(window: &Panel, view: &PanelView) {
    window.set_bulb_name(view.bulb_name.clone().into());
    let notice = match (&view.setup, &view.notice) {
        // As telas da configuração inicial chegam na #32.
        (Some(_), _) => "Configuração inicial em andamento.".to_owned(),
        (None, Some(notice)) => notice.clone(),
        (None, None) => String::new(),
    };
    window.set_notice(notice.into());
    window.set_controls_enabled(view.controls_enabled);
    window.set_power_on(view.power_on);
    window.set_colour_tab(view.tab == Tab::Colour);
    window.set_marker_x(view.marker.x);
    window.set_marker_y(view.marker.y);
    window.set_brightness(f32::from(view.brightness_percent.max(1) - 1) / 99.0);
    window.set_brightness_percent(i32::from(view.brightness_percent));
    window.set_temperature(f32::from(view.temperature_percent) / 100.0);
    window.set_full_colour(colour(view.full_colour));
    let favorites: Vec<slint::Color> = view.favorites.iter().copied().map(colour).collect();
    window.set_favorites(ModelRc::new(VecModel::from(favorites)));
}

fn colour(rgb: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(rgb.r, rgb.g, rgb.b)
}
