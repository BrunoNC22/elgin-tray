//! Abre o painel numa tela da configuração inicial, com dados de exemplo, para conferir o visual
//! sem passar pelo login de verdade:
//!
//! `cargo run -p elgin-panel --example setup_preview -- code|failed|qr|choose|working|done|lost`

use elgin_ipc::messages::{SetupDeviceDto, SetupStateDto};
use elgin_panel::Panel;
use elgin_panel::runner::render_setup;
use elgin_panel::setup_view::SetupView;
use slint::ComponentHandle;

fn main() -> Result<(), slint::PlatformError> {
    let device = |id: &str, name: &str| SetupDeviceDto {
        id: id.to_owned(),
        name: name.to_owned(),
        product_name: "10W Smart color".to_owned(),
    };
    let state = match std::env::args().nth(1).as_deref().unwrap_or("choose") {
        "code" => SetupStateDto::AwaitingUserCode,
        "failed" => SetupStateDto::Failed {
            reason: "o QR code expirou; gere outro".to_owned(),
        },
        "qr" => SetupStateDto::WaitingConfirmation {
            qr_content: "tuyaSmart--qrLogin?token=exemplo".to_owned(),
        },
        "working" => SetupStateDto::Discovering,
        "done" => SetupStateDto::Done {
            bulb_name: "Lâmpada do quarto".to_owned(),
            found_on_network: true,
        },
        "lost" => SetupStateDto::Done {
            bulb_name: "Lâmpada do quarto".to_owned(),
            found_on_network: false,
        },
        _ => SetupStateDto::ChooseDevice {
            devices: vec![
                device("a1", "Lâmpada do quarto"),
                device("a2", "Lâmpada da sala"),
            ],
        },
    };
    let window = Panel::new()?;
    window.set_bulb_name("elgin-tray".into());
    render_setup(&window, Some(SetupView::of(&state)));
    window.on_setup_closed(|| {
        let _ = slint::quit_event_loop();
    });
    window.run()
}
