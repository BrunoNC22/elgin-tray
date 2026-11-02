use crate::brightness::Brightness;
use crate::error::{DomainError, ensure_range};
use crate::percent::permille_to_percent;

/// Converte uma intensidade de 0.0 a 1.0 num canal de 8 bits (arredondado, limitado a 0–255).
pub(crate) fn channel(intensity: f64) -> u8 {
    // Depois do clamp o valor está em 0.0..=255.0, e `as` de float para inteiro satura em Rust.
    (intensity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Matiz em graus (0–359). 360 equivale a 0: o valor é normalizado ao criar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hue(u16);

impl Hue {
    pub fn new(degrees: u16) -> Self {
        Self(degrees % 360)
    }

    pub fn degrees(self) -> u16 {
        self.0
    }
}

/// Saturação em permil: 0 é branco (sem cor) e 1000 é a cor pura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Saturation(u16);

impl Saturation {
    pub const NONE: Self = Self(0);
    pub const FULL: Self = Self(1000);

    pub fn try_from_permille(permille: u16) -> Result<Self, DomainError> {
        ensure_range("saturação", permille.into(), 0, Self::FULL.0.into())?;
        Ok(Self(permille))
    }

    pub fn clamped_from_permille(permille: u16) -> Self {
        Self(permille.min(Self::FULL.0))
    }

    pub fn permille(self) -> u16 {
        self.0
    }

    /// Porcentagem arredondada (meio para cima), de 0 a 100.
    pub fn percent(self) -> u8 {
        permille_to_percent(self.0)
    }
}

/// Cor no espaço HSV, como a lâmpada usa no modo cor. O "V" é o brilho da cor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hsv {
    hue: Hue,
    saturation: Saturation,
    value: Brightness,
}

impl Hsv {
    pub fn new(hue: Hue, saturation: Saturation, value: Brightness) -> Self {
        Self {
            hue,
            saturation,
            value,
        }
    }

    pub fn hue(self) -> Hue {
        self.hue
    }

    pub fn saturation(self) -> Saturation {
        self.saturation
    }

    pub fn value(self) -> Brightness {
        self.value
    }

    /// A mesma cor com outro brilho.
    pub fn with_value(self, value: Brightness) -> Self {
        Self { value, ..self }
    }

    /// Converte para RGB aplicando o brilho (`value`).
    pub fn to_rgb(self) -> Rgb {
        let saturation = f64::from(self.saturation.permille()) / 1000.0;
        let value = f64::from(self.value.permille()) / 1000.0;
        let sector = f64::from(self.hue.degrees()) / 60.0;

        let chroma = value * saturation;
        let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
        let (r, g, b) = match self.hue.degrees() / 60 {
            0 => (chroma, second, 0.0),
            1 => (second, chroma, 0.0),
            2 => (0.0, chroma, second),
            3 => (0.0, second, chroma),
            4 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let white = value - chroma;
        Rgb::new(channel(r + white), channel(g + white), channel(b + white))
    }
}

/// Cor RGB de 8 bits por canal, para desenhar na interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hsv(hue: u16, saturation: u16, value: u16) -> Hsv {
        Hsv::new(
            Hue::new(hue),
            Saturation::try_from_permille(saturation).unwrap(),
            Brightness::try_from_permille(value).unwrap(),
        )
    }

    #[test]
    fn hue_keeps_values_below_360() {
        assert_eq!(Hue::new(0).degrees(), 0);
        assert_eq!(Hue::new(359).degrees(), 359);
    }

    #[test]
    fn hue_360_is_the_same_as_0() {
        assert_eq!(Hue::new(360), Hue::new(0));
        assert_eq!(Hue::new(725).degrees(), 5);
    }

    #[test]
    fn saturation_accepts_0_to_1000_and_rejects_above() {
        assert_eq!(Saturation::try_from_permille(0), Ok(Saturation::NONE));
        assert_eq!(Saturation::try_from_permille(1000), Ok(Saturation::FULL));
        assert_eq!(
            Saturation::try_from_permille(1001),
            Err(DomainError::OutOfRange {
                quantity: "saturação",
                value: 1001,
                min: 0,
                max: 1000
            })
        );
        assert_eq!(Saturation::clamped_from_permille(9999), Saturation::FULL);
    }

    #[test]
    fn saturation_percentage_rounds_half_up() {
        assert_eq!(Saturation::try_from_permille(605).unwrap().percent(), 61);
        assert_eq!(Saturation::try_from_permille(604).unwrap().percent(), 60);
    }

    #[test]
    fn hsv_exposes_its_parts_and_changes_only_the_value() {
        let colour = hsv(200, 800, 500);
        let brighter = colour.with_value(Brightness::MAX);

        assert_eq!(colour.hue().degrees(), 200);
        assert_eq!(colour.saturation().permille(), 800);
        assert_eq!(colour.value().permille(), 500);
        assert_eq!(brighter.hue(), colour.hue());
        assert_eq!(brighter.saturation(), colour.saturation());
        assert_eq!(brighter.value(), Brightness::MAX);
    }

    #[test]
    fn converts_primary_and_secondary_colours_to_rgb() {
        assert_eq!(hsv(0, 1000, 1000).to_rgb(), Rgb::new(255, 0, 0));
        assert_eq!(hsv(60, 1000, 1000).to_rgb(), Rgb::new(255, 255, 0));
        assert_eq!(hsv(120, 1000, 1000).to_rgb(), Rgb::new(0, 255, 0));
        assert_eq!(hsv(180, 1000, 1000).to_rgb(), Rgb::new(0, 255, 255));
        assert_eq!(hsv(240, 1000, 1000).to_rgb(), Rgb::new(0, 0, 255));
        assert_eq!(hsv(300, 1000, 1000).to_rgb(), Rgb::new(255, 0, 255));
    }

    #[test]
    fn converts_an_intermediate_hue() {
        assert_eq!(hsv(30, 1000, 1000).to_rgb(), Rgb::new(255, 128, 0));
    }

    #[test]
    fn no_saturation_is_white_or_grey() {
        assert_eq!(hsv(123, 0, 1000).to_rgb(), Rgb::new(255, 255, 255));
        assert_eq!(hsv(123, 0, 500).to_rgb(), Rgb::new(128, 128, 128));
    }

    #[test]
    fn partial_saturation_mixes_with_white() {
        assert_eq!(hsv(0, 500, 1000).to_rgb(), Rgb::new(255, 128, 128));
    }

    #[test]
    fn value_scales_the_colour() {
        assert_eq!(hsv(0, 1000, 500).to_rgb(), Rgb::new(128, 0, 0));
        assert_eq!(hsv(0, 1000, 10).to_rgb(), Rgb::new(3, 0, 0));
    }
}
