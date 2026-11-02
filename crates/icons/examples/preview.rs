//! Gera uma folha com todos os estados do ícone, em fundo claro e escuro, para revisar o visual.
//!
//! `cargo run -p elgin-icons --example preview -- docs/images/tray-icons-preview.png`

use elgin_application::tray::{IconSpec, Indicator};
use elgin_domain::{ColorTemperature, Rgb};
use elgin_icons::{Rgba, render};
use tiny_skia::{Color, IntSize, Pixmap, PixmapPaint, Transform};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tray-icons-preview.png".to_owned());
    let specs = [
        IconSpec::On(Rgb::new(255, 0, 0)),
        IconSpec::On(Rgb::new(0, 90, 255)),
        IconSpec::On(ColorTemperature::WARMEST.display_colour()),
        IconSpec::On(ColorTemperature::COOLEST.display_colour()),
        IconSpec::Off,
        IconSpec::Unavailable(Indicator::Warning),
        IconSpec::Unavailable(Indicator::Error),
    ];
    let backgrounds = [
        Color::from_rgba8(243, 243, 243, 255),
        Color::from_rgba8(32, 32, 32, 255),
    ];
    let sizes = [16u32, 24, 32, 64];
    let cell = 140;
    let mut sheet = Pixmap::new(cell * specs.len() as u32, 90 * backgrounds.len() as u32).unwrap();
    for (row, background) in backgrounds.iter().enumerate() {
        let mut band = Pixmap::new(sheet.width(), 90).unwrap();
        band.fill(*background);
        sheet.draw_pixmap(
            0,
            (row * 90) as i32,
            band.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        for (column, spec) in specs.iter().enumerate() {
            let mut x = (column as u32 * cell + 6) as i32;
            for size in sizes {
                let icon = to_pixmap(&render(spec, size));
                let y = (row * 90 + 45) as i32 - size as i32 / 2;
                sheet.draw_pixmap(
                    x,
                    y,
                    icon.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                x += size as i32 + 4;
            }
        }
    }
    sheet.save_png(&out).unwrap();
    println!("pré-visualização gravada em {out}");
}

/// RGBA comum (alfa não pré-multiplicado) → pixmap do tiny-skia (pré-multiplicado).
fn to_pixmap(icon: &Rgba) -> Pixmap {
    let data = icon
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u16::from(p[3]);
            let premultiply = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
            [
                premultiply(p[0]),
                premultiply(p[1]),
                premultiply(p[2]),
                p[3],
            ]
        })
        .collect();
    Pixmap::from_vec(data, IntSize::from_wh(icon.width, icon.height).unwrap()).unwrap()
}
