//! Renderização do ícone da bandeja (RGBA) a partir do estado.
//!
//! O desenho é feito numa grade de 64×64 unidades e escalado para o tamanho pedido (a bandeja usa
//! 16–32 px; telas com DPI alto pedem mais).

use elgin_application::tray::{IconSpec, Indicator};
use elgin_domain::Rgb;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Tamanhos gerados para a bandeja (o sistema escolhe o melhor para o DPI).
pub const SIZES: [u32; 5] = [16, 20, 24, 32, 64];

/// Imagem RGBA de 8 bits por canal, sem alfa pré-multiplicado (o formato que as bandejas aceitam).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, linha por linha.
    pub data: Vec<u8>,
}

impl Rgba {
    /// O pixel `(x, y)` como `[r, g, b, a]`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * self.width + x) * 4) as usize;
        [
            self.data[index],
            self.data[index + 1],
            self.data[index + 2],
            self.data[index + 3],
        ]
    }
}

/// Desenha o ícone de `spec` com `size` × `size` pixels.
pub fn render(spec: &IconSpec, size: u32) -> Rgba {
    let size = size.max(1);
    let mut pixmap = Pixmap::new(size, size).expect("tamanho maior que zero");
    let scale = size as f32 / 64.0;
    let transform = Transform::from_scale(scale, scale);
    let glass = glass_path();

    match spec {
        IconSpec::On(colour) => {
            pixmap.fill_path(
                &glass,
                &paint(*colour, 255),
                FillRule::Winding,
                transform,
                None,
            );
            stroke(&mut pixmap, &glass, OUTLINE_DARK, 2.5, transform);
        }
        IconSpec::Off => stroke(&mut pixmap, &glass, OUTLINE_GREY, 4.0, transform),
        IconSpec::Unavailable(_) => {
            pixmap.fill_path(
                &glass,
                &paint(UNAVAILABLE_GREY, 220),
                FillRule::Winding,
                transform,
                None,
            );
            stroke(&mut pixmap, &glass, OUTLINE_GREY, 2.5, transform);
        }
    }
    draw_base(&mut pixmap, transform);
    if let IconSpec::Unavailable(indicator) = spec {
        let colour = match indicator {
            Indicator::Warning => WARNING_ORANGE,
            Indicator::Error => ERROR_RED,
        };
        let dot = PathBuilder::from_circle(52.0, 52.0, 11.0).expect("círculo válido");
        pixmap.fill_path(
            &dot,
            &paint(colour, 255),
            FillRule::Winding,
            transform,
            None,
        );
        stroke(&mut pixmap, &dot, Rgb::new(255, 255, 255), 2.5, transform);
    }

    let data = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let straight = pixel.demultiply();
            [
                straight.red(),
                straight.green(),
                straight.blue(),
                straight.alpha(),
            ]
        })
        .collect();
    Rgba {
        width: size,
        height: size,
        data,
    }
}

/// Contorno escuro da lâmpada ligada (aparece mesmo com cores claras em barras claras).
const OUTLINE_DARK: Rgb = Rgb::new(48, 48, 48);
/// Cinza médio: visível tanto em barras claras quanto escuras.
const OUTLINE_GREY: Rgb = Rgb::new(140, 140, 140);
const UNAVAILABLE_GREY: Rgb = Rgb::new(158, 158, 158);
const WARNING_ORANGE: Rgb = Rgb::new(245, 150, 30);
const ERROR_RED: Rgb = Rgb::new(220, 40, 40);

/// Vidro da lâmpada: um único contorno em forma de pera (bulbo redondo afinando até a rosca), para
/// o traço não cruzar o desenho por dentro.
fn glass_path() -> Path {
    let mut builder = PathBuilder::new();
    builder.move_to(25.0, 46.0);
    builder.cubic_to(25.0, 40.0, 13.0, 34.0, 13.0, 24.0);
    builder.cubic_to(13.0, 13.0, 22.0, 5.0, 32.0, 5.0);
    builder.cubic_to(42.0, 5.0, 51.0, 13.0, 51.0, 24.0);
    builder.cubic_to(51.0, 34.0, 39.0, 40.0, 39.0, 46.0);
    builder.close();
    builder.finish().expect("caminho válido")
}

/// Rosca da lâmpada: dois anéis cinza.
fn draw_base(pixmap: &mut Pixmap, transform: Transform) {
    for (y, height) in [(49.0, 5.0), (55.5, 4.5)] {
        let rect = Rect::from_xywh(25.0, y, 14.0, height).expect("retângulo válido");
        pixmap.fill_rect(rect, &paint(OUTLINE_GREY, 255), transform, None);
    }
}

fn stroke(pixmap: &mut Pixmap, path: &Path, colour: Rgb, width: f32, transform: Transform) {
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint(colour, 255), &stroke, transform, None);
}

fn paint(colour: Rgb, alpha: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(colour.r, colour.g, colour.b, alpha);
    paint.anti_alias = true;
    paint
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ponto da grade de 64 unidades no ícone de 64 px.
    fn at(icon: &Rgba, x: u32, y: u32) -> [u8; 4] {
        icon.pixel(x * icon.width / 64, y * icon.height / 64)
    }

    fn close(actual: [u8; 4], expected: [u8; 4]) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(&a, e)| a.abs_diff(e) <= 8)
    }

    const RED: Rgb = Rgb::new(255, 0, 0);

    #[test]
    fn has_the_requested_size() {
        for size in SIZES {
            let icon = render(&IconSpec::Off, size);
            assert_eq!((icon.width, icon.height), (size, size));
            assert_eq!(icon.data.len(), (size * size * 4) as usize);
        }
    }

    #[test]
    fn an_on_bulb_is_filled_with_its_colour() {
        let icon = render(&IconSpec::On(RED), 64);

        assert!(
            close(at(&icon, 32, 24), [255, 0, 0, 255]),
            "{:?}",
            at(&icon, 32, 24)
        );
        assert_eq!(at(&icon, 1, 1)[3], 0, "fora da lâmpada é transparente");
    }

    #[test]
    fn an_off_bulb_is_only_outlined() {
        let icon = render(&IconSpec::Off, 64);

        assert_eq!(at(&icon, 32, 24)[3], 0, "o vidro fica vazio");
        assert!(
            at(&icon, 32, 6)[3] > 200,
            "o contorno aparece: {:?}",
            at(&icon, 32, 6)
        );
        assert!(at(&icon, 32, 56)[3] > 200, "a base aparece");
    }

    #[test]
    fn an_unavailable_bulb_is_grey_with_a_coloured_dot() {
        let warning = render(&IconSpec::Unavailable(Indicator::Warning), 64);
        let error = render(&IconSpec::Unavailable(Indicator::Error), 64);

        let [r, g, b, a] = at(&warning, 26, 20);
        assert!(
            a > 0 && r.abs_diff(g) < 10 && g.abs_diff(b) < 10,
            "vidro cinza: {:?}",
            [r, g, b, a]
        );
        let [r, g, b, _] = at(&warning, 52, 52);
        assert!(
            r > 200 && g > 100 && g < 200 && b < 80,
            "ponto laranja: {:?}",
            [r, g, b]
        );
        let [r, g, b, _] = at(&error, 52, 52);
        assert!(
            r > 180 && g < 80 && b < 80,
            "ponto vermelho: {:?}",
            [r, g, b]
        );
    }

    #[test]
    fn the_drawing_scales_down_to_16_pixels() {
        let icon = render(&IconSpec::On(RED), 16);

        assert!(
            close(at(&icon, 32, 24), [255, 0, 0, 255]),
            "{:?}",
            at(&icon, 32, 24)
        );
        assert_eq!(icon.pixel(0, 0)[3], 0);
    }
}
