//! A imagem do disco de cores (gerada em Rust, desenhada pelo Slint).

use elgin_domain::Brightness;

use crate::disc::{DiscPoint, colour_at};

/// Imagem quadrada RGBA (alfa não pré-multiplicado).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscImage {
    pub size: u32,
    pub rgba: Vec<u8>,
}

impl DiscImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * self.size + x) * 4) as usize;
        [
            self.rgba[index],
            self.rgba[index + 1],
            self.rgba[index + 2],
            self.rgba[index + 3],
        ]
    }
}

/// Desenha o disco com `size` × `size` pixels: cada ponto com a cor de [`colour_at`] no brilho
/// máximo e a borda suavizada (anti-aliasing).
pub fn render_disc(size: u32) -> DiscImage {
    let size = size.max(1);
    let radius = size as f32 / 2.0;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            // Centro do pixel, normalizado para o disco de raio 1.
            let point = DiscPoint {
                x: (x as f32 + 0.5 - radius) / radius,
                y: (y as f32 + 0.5 - radius) / radius,
            };
            // Cobertura do pixel pela borda: 1 dentro, 0 fora, suave na faixa de 1 pixel.
            let coverage = ((1.0 - point.x.hypot(point.y)) * radius + 0.5).clamp(0.0, 1.0);
            let colour = colour_at(point).with_brightness(Brightness::MAX).to_rgb();
            rgba.extend_from_slice(&[
                colour.r,
                colour.g,
                colour.b,
                (coverage * 255.0).round() as u8,
            ]);
        }
    }
    DiscImage { size, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_the_requested_size() {
        let image = render_disc(64);

        assert_eq!(image.size, 64);
        assert_eq!(image.rgba.len(), 64 * 64 * 4);
    }

    #[test]
    fn the_centre_is_white_and_the_corners_are_transparent() {
        let image = render_disc(101);

        let [r, g, b, a] = image.pixel(50, 50);
        assert!(
            r > 245 && g > 245 && b > 245 && a == 255,
            "{:?}",
            [r, g, b, a]
        );
        assert_eq!(image.pixel(0, 0)[3], 0);
        assert_eq!(image.pixel(100, 100)[3], 0);
    }

    #[test]
    fn the_edge_has_the_pure_colour_of_its_angle() {
        let image = render_disc(101);

        let top = image.pixel(50, 2);
        assert!(
            top[0] > 240 && top[1] < 30 && top[2] < 30 && top[3] == 255,
            "topo vermelho: {top:?}"
        );
        let bottom = image.pixel(50, 98);
        assert!(
            bottom[0] < 30 && bottom[1] > 240 && bottom[2] > 240,
            "baixo ciano: {bottom:?}"
        );
    }

    #[test]
    fn the_border_is_smoothed() {
        let image = render_disc(101);
        let partial = image
            .rgba
            .chunks(4)
            .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
            .count();

        // A borda suave é uma faixa fina em volta do círculo (perímetro de ~317 px).
        assert!((100..600).contains(&partial), "{partial} pixels parciais");
    }
}
