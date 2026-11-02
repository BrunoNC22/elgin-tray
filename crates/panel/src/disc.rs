//! Matemática do disco de cores: ponto no disco ↔ matiz e saturação.
//!
//! Coordenadas normalizadas: centro em (0, 0), borda no raio 1, `y` crescendo para baixo (como na
//! tela). O matiz é o ângulo, com 0° (vermelho) no topo e aumentando no sentido horário; a saturação
//! é a distância do centro (centro = branco, borda = cor pura).

use elgin_domain::{FavoriteColour, Hue, Saturation};

/// Um ponto no disco, em coordenadas normalizadas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiscPoint {
    pub x: f32,
    pub y: f32,
}

/// A cor (matiz e saturação) sob um ponto. Pontos fora do disco contam como a borda.
pub fn colour_at(point: DiscPoint) -> FavoriteColour {
    let distance = point.x.hypot(point.y).min(1.0);
    // atan2(x, -y): 0 no topo, crescendo no sentido horário (y da tela cresce para baixo).
    let degrees = point.x.atan2(-point.y).to_degrees().rem_euclid(360.0);
    FavoriteColour::new(
        Hue::new(degrees.round() as u16),
        Saturation::clamped_from_permille((distance * 1000.0).round() as u16),
    )
}

/// Onde fica o marcador de uma cor.
pub fn point_of(colour: FavoriteColour) -> DiscPoint {
    let angle = f32::from(colour.hue().degrees()).to_radians();
    let distance = f32::from(colour.saturation().permille()) / 1000.0;
    DiscPoint {
        x: distance * angle.sin(),
        y: -distance * angle.cos(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colour(point: (f32, f32)) -> (u16, u16) {
        let colour = colour_at(DiscPoint {
            x: point.0,
            y: point.1,
        });
        (colour.hue().degrees(), colour.saturation().permille())
    }

    fn near(a: DiscPoint, b: DiscPoint) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
    }

    #[test]
    fn cardinal_points_on_the_edge() {
        assert_eq!(colour((0.0, -1.0)), (0, 1000), "topo = vermelho");
        assert_eq!(colour((1.0, 0.0)), (90, 1000), "direita");
        assert_eq!(colour((0.0, 1.0)), (180, 1000), "baixo");
        assert_eq!(colour((-1.0, 0.0)), (270, 1000), "esquerda");
    }

    #[test]
    fn the_centre_is_white() {
        assert_eq!(colour((0.0, 0.0)).1, 0);
    }

    #[test]
    fn halfway_is_half_saturation() {
        assert_eq!(colour((0.0, -0.5)), (0, 500));
    }

    #[test]
    fn points_outside_the_disc_stick_to_the_edge() {
        assert_eq!(colour((0.0, -3.0)), (0, 1000));
        assert_eq!(colour((2.0, 2.0)).1, 1000);
    }

    #[test]
    fn point_and_colour_round_trip() {
        for (hue, saturation) in [(0, 1000), (45, 700), (200, 300), (315, 1000)] {
            let colour = FavoriteColour::new(
                Hue::new(hue),
                Saturation::try_from_permille(saturation).unwrap(),
            );
            let point = point_of(colour);
            assert_eq!(
                colour_at(point),
                colour,
                "{hue}° {saturation}‰ em {point:?}"
            );
        }
    }

    #[test]
    fn red_is_at_the_top() {
        let red = FavoriteColour::new(Hue::new(0), Saturation::FULL);

        assert!(near(point_of(red), DiscPoint { x: 0.0, y: -1.0 }));
    }
}
