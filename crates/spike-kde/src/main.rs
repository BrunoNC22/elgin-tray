//! Spike #26 (descartável). Uso: SPIKE_PANEL=wayland|x11|kwin cargo run -p spike-kde
//! Clique esquerdo no ícone: registra (x, y) e abre `elgin-tray panel --anchor x,y,1,1`.

use std::time::Instant;

struct Spike {
    started: Instant,
    clicks: usize,
}

/// Cada clique usa o próximo modo: (SPIKE_PANEL, SPIKE_X11_TYPE).
const MODES: [(&str, &str); 4] = [
    ("wayland", ""),
    ("x11", "popup"),
    ("x11", "utility"),
    ("kwin", ""),
];

impl ksni::Tray for Spike {
    fn id(&self) -> String {
        "elgin-tray-spike".into()
    }

    fn title(&self) -> String {
        "Spike #26".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Círculo laranja 32x32 em ARGB32 big-endian.
        let size = 32;
        let mut data = Vec::with_capacity(size * size * 4);
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
                let inside = dx * dx + dy * dy <= 15.0 * 15.0;
                data.extend_from_slice(if inside {
                    &[255, 255, 160, 0]
                } else {
                    &[0, 0, 0, 0]
                });
            }
        }
        vec![ksni::Icon {
            width: size as i32,
            height: size as i32,
            data,
        }]
    }

    fn activate(&mut self, x: i32, y: i32) {
        let (mode, x11_type) = MODES[self.clicks % MODES.len()];
        self.clicks += 1;
        eprintln!(
            "[{:>7.3}s] activate x={x} y={y} modo={mode} {x11_type}",
            self.started.elapsed().as_secs_f32()
        );
        let exe = std::env::current_exe()
            .unwrap()
            .with_file_name("elgin-tray");
        let mut command = std::process::Command::new(exe);
        command
            .args(["panel", "--anchor", &format!("{x},{y},1,1")])
            .env("SPIKE_PANEL", mode)
            .env("SPIKE_X11_TYPE", x11_type)
            .env("SPIKE_CLICK_EPOCH_MS", epoch_ms().to_string());
        if mode == "x11" {
            command.env_remove("WAYLAND_DISPLAY");
        }
        if let Err(error) = command.spawn() {
            eprintln!("não abriu o painel: {error}");
        }
    }

    fn secondary_activate(&mut self, x: i32, y: i32) {
        eprintln!("secondary_activate x={x} y={y}");
    }

    fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
        eprintln!("scroll delta={delta} {orientation:?}");
    }
}

fn epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    use ksni::TrayMethods;
    let _handle = Spike {
        started: Instant::now(),
        clicks: 0,
    }
    .spawn()
    .await
    .expect("não registrou o ícone");
    eprintln!("ícone registrado; clique nele (Ctrl+C para sair)");
    std::future::pending::<()>().await;
}
