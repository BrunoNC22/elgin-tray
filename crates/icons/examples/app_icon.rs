//! Gera o ícone do aplicativo (lâmpada acesa, 256×256) usado nos pacotes:
//!
//! `cargo run -p elgin-icons --example app_icon -- assets/elgin-tray.png`

use elgin_application::tray::IconSpec;
use elgin_domain::Rgb;
use elgin_icons::render;
use tiny_skia::{IntSize, Pixmap};

/// Amarelo de lâmpada acesa.
const LIT: Rgb = Rgb::new(255, 196, 64);
const SIZE: u32 = 256;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "elgin-tray.png".to_owned());
    let icon = render(&IconSpec::On(LIT), SIZE);
    // O tiny-skia guarda o alfa pré-multiplicado.
    let data = icon
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u16::from(p[3]);
            let premultiply = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
            [premultiply(p[0]), premultiply(p[1]), premultiply(p[2]), p[3]]
        })
        .collect();
    let pixmap = Pixmap::from_vec(data, IntSize::from_wh(SIZE, SIZE).unwrap()).unwrap();
    pixmap.save_png(&out).unwrap();
    println!("ícone gravado em {out}");
}
