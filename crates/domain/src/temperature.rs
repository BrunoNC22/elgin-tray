use crate::colour::{Rgb, channel};
use crate::error::{DomainError, ensure_range};
use crate::percent::{percent_to_permille, permille_to_percent};

const QUANTITY: &str = "temperatura";

/// Faixa de temperatura de cor suposta para a lâmpada, em kelvin.
const WARMEST_KELVIN: f64 = 2700.0;
const COOLEST_KELVIN: f64 = 6500.0;

/// Temperatura da luz branca, em permil: 0 é o mais quente (amarelado) e 1000 o mais frio (azulado).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColorTemperature(u16);

impl ColorTemperature {
    pub const WARMEST: Self = Self(0);
    pub const COOLEST: Self = Self(1000);

    /// Cria a partir do valor em permil, recusando valores acima de 1000.
    pub fn try_from_permille(permille: u16) -> Result<Self, DomainError> {
        ensure_range(QUANTITY, permille.into(), 0, Self::COOLEST.0.into())?;
        Ok(Self(permille))
    }

    /// Cria a partir do valor em permil, limitando a 1000.
    pub fn clamped_from_permille(permille: u16) -> Self {
        Self(permille.min(Self::COOLEST.0))
    }

    /// Cria a partir de uma porcentagem de 0 (quente) a 100 (frio).
    pub fn from_percent(percent: u8) -> Result<Self, DomainError> {
        ensure_range("porcentagem de temperatura", percent.into(), 0, 100)?;
        Ok(Self(percent_to_permille(percent)))
    }

    pub fn permille(self) -> u16 {
        self.0
    }

    /// Porcentagem arredondada (meio para cima), de 0 a 100.
    pub fn percent(self) -> u8 {
        permille_to_percent(self.0)
    }

    /// Cor aproximada desta luz branca, para ícones e gradientes na interface.
    ///
    /// Supõe que a faixa da lâmpada vai de 2700 K (quente) a 6500 K (frio) e converte a temperatura
    /// em RGB com a aproximação de Tanner Helland. É só uma cor de exibição, não uma medida.
    pub fn display_colour(self) -> Rgb {
        let fraction = f64::from(self.0) / 1000.0;
        let kelvin = WARMEST_KELVIN + fraction * (COOLEST_KELVIN - WARMEST_KELVIN);
        // A fórmula trabalha em centenas de kelvin; até 6600 K o vermelho fica no máximo.
        let t = kelvin / 100.0;
        let green = 99.470_802_586_1 * t.ln() - 161.119_568_166_1;
        let blue = 138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7;
        Rgb::new(255, channel(green / 255.0), channel(blue / 255.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_0_to_1000() {
        assert_eq!(
            ColorTemperature::try_from_permille(0),
            Ok(ColorTemperature::WARMEST)
        );
        assert_eq!(
            ColorTemperature::try_from_permille(1000),
            Ok(ColorTemperature::COOLEST)
        );
    }

    #[test]
    fn rejects_values_above_1000() {
        assert_eq!(
            ColorTemperature::try_from_permille(1001),
            Err(DomainError::OutOfRange {
                quantity: "temperatura",
                value: 1001,
                min: 0,
                max: 1000
            })
        );
    }

    #[test]
    fn clamps_values_above_1000() {
        assert_eq!(
            ColorTemperature::clamped_from_permille(4000),
            ColorTemperature::COOLEST
        );
        assert_eq!(ColorTemperature::clamped_from_permille(300).permille(), 300);
    }

    #[test]
    fn converts_from_and_to_percentage() {
        assert_eq!(
            ColorTemperature::from_percent(0),
            Ok(ColorTemperature::WARMEST)
        );
        assert_eq!(
            ColorTemperature::from_percent(100),
            Ok(ColorTemperature::COOLEST)
        );
        assert!(ColorTemperature::from_percent(101).is_err());
        assert_eq!(
            ColorTemperature::try_from_permille(445).unwrap().percent(),
            45
        );
        assert_eq!(
            ColorTemperature::try_from_permille(444).unwrap().percent(),
            44
        );
        for percent in 0..=100 {
            assert_eq!(
                ColorTemperature::from_percent(percent).unwrap().percent(),
                percent
            );
        }
    }

    #[test]
    fn warmest_white_looks_orange() {
        let colour = ColorTemperature::WARMEST.display_colour();

        assert_eq!(colour.r, 255);
        assert!(colour.g > colour.b, "verde deve passar o azul: {colour:?}");
        assert!(colour.b < 120, "azul deve ser baixo: {colour:?}");
    }

    #[test]
    fn coolest_white_looks_almost_white() {
        let colour = ColorTemperature::COOLEST.display_colour();

        assert_eq!(colour.r, 255);
        assert!(
            colour.g >= 245 && colour.b >= 245,
            "deve ser quase branco: {colour:?}"
        );
    }

    #[test]
    fn blue_increases_as_the_light_gets_cooler() {
        let blues: Vec<u8> = (0..=10)
            .map(|step| ColorTemperature::try_from_permille(step * 100).unwrap())
            .map(|temperature| temperature.display_colour().b)
            .collect();

        assert!(blues.windows(2).all(|pair| pair[0] <= pair[1]), "{blues:?}");
    }
}
