//! Spike #26 (descartável): modos de posicionamento do painel no KDE, escolhidos por `SPIKE_PANEL`.
//! - `wayland`: nada (o KWin decide);
//! - `x11`: XWayland, tipo de janela `SPIKE_X11_TYPE` (popup|utility|dialog|dropdown) e posição na criação;
//! - `kwin`: Wayland nativo com app_id próprio; um script do KWin (DBus) posiciona a janela.

use crate::placement::{Edge, Rect, panel_position};
use slint::winit_030::winit;

const SIZE: (i32, i32) = (320, 490);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Onde o painel deveria ficar (barra em cima, área de trabalho de `SPIKE_WORK`).
fn target(anchor: Option<Rect>) -> (i32, i32) {
    let work: Vec<i32> = env("SPIKE_WORK")
        .split(',')
        .filter_map(|n| n.parse().ok())
        .collect();
    let work = match work[..] {
        [x, y, width, height] => Rect {
            x,
            y,
            width,
            height,
        },
        _ => Rect {
            x: 0,
            y: 46,
            width: 1920,
            height: 1034,
        },
    };
    let edge = match env("SPIKE_EDGE").as_str() {
        "bottom" => Edge::Bottom,
        "left" => Edge::Left,
        "right" => Edge::Right,
        _ => Edge::Top,
    };
    panel_position(anchor, SIZE, work, edge)
}

pub(crate) fn elapsed_since_click() -> String {
    let click: u128 = env("SPIKE_CLICK_EPOCH_MS").parse().unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{} ms desde o clique", now.saturating_sub(click))
}

pub(crate) fn before_window(anchor: Option<Rect>) {
    let (x, y) = target(anchor);
    eprintln!(
        "[spike] modo={} anchor={anchor:?} alvo=({x},{y})",
        env("SPIKE_PANEL")
    );
    match env("SPIKE_PANEL").as_str() {
        "x11" => {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            let kind = match env("SPIKE_X11_TYPE").as_str() {
                "utility" => WindowType::Utility,
                "dialog" => WindowType::Dialog,
                "dropdown" => WindowType::DropdownMenu,
                _ => WindowType::PopupMenu,
            };
            let result = slint::BackendSelector::new()
                .with_winit_window_attributes_hook(move |attributes| {
                    attributes
                        .with_x11_window_type(vec![kind])
                        .with_position(winit::dpi::PhysicalPosition::new(x, y))
                })
                .select();
            eprintln!("[spike] backend x11: {result:?}");
        }
        "kwin" => {
            use winit::platform::wayland::WindowAttributesExtWayland;
            let result = slint::BackendSelector::new()
                .with_winit_window_attributes_hook(|attributes| {
                    attributes.with_name("elgin-tray-panel", "elgin-tray-panel")
                })
                .select();
            eprintln!("[spike] backend wayland/kwin: {result:?}");
            load_kwin_script(x, y);
        }
        _ => {}
    }
}

fn load_kwin_script(x: i32, y: i32) {
    let started = std::time::Instant::now();
    let path = format!("{}/elgin-tray-panel.js", env("XDG_RUNTIME_DIR"));
    let script = format!(
        r#"function place(w) {{
    if (w.resourceClass !== "elgin-tray-panel") return;
    const g = w.frameGeometry;
    w.frameGeometry = {{x: {x}, y: {y}, width: g.width, height: g.height}};
    w.skipTaskbar = true; w.skipPager = true; w.skipSwitcher = true; w.keepAbove = true;
}}
workspace.windowList().forEach(place);
workspace.windowAdded.connect(place);
"#
    );
    std::fs::write(&path, script).unwrap();
    let qdbus = |args: &[&str]| {
        std::process::Command::new("qdbus6")
            .args(["org.kde.KWin"])
            .args(args)
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    qdbus(&[
        "/Scripting",
        "org.kde.kwin.Scripting.unloadScript",
        "elgin-tray-panel",
    ]);
    let id = qdbus(&[
        "/Scripting",
        "org.kde.kwin.Scripting.loadScript",
        &path,
        "elgin-tray-panel",
    ]);
    qdbus(&[&format!("/Scripting/Script{id}"), "org.kde.kwin.Script.run"]);
    eprintln!(
        "[spike] script do KWin id={id} carregado em {:?}",
        started.elapsed()
    );
}
