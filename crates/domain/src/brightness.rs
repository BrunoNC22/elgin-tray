use crate::error::{DomainError, ensure_range};
use crate::percent::{percent_to_permille, permille_to_percent};

const QUANTITY: &str = "brilho";

/// Brilho da lâmpada, guardado em permil (10–1000) para não perder a precisão que a lâmpada informa.
///
/// Na interface o brilho aparece em porcentagem (1–100%). O mínimo é 1%: a lâmpada não apaga pelo
/// brilho, só pelo liga/desliga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Brightness(u16);

impl Brightness {
    pub const MIN: Self = Self(10);
    pub const MAX: Self = Self(1000);

    /// Cria a partir do valor em permil, recusando valores fora de 10–1000.
    pub fn try_from_permille(permille: u16) -> Result<Self, DomainError> {
        ensure_range(
            QUANTITY,
            permille.into(),
            Self::MIN.0.into(),
            Self::MAX.0.into(),
        )?;
        Ok(Self(permille))
    }

    /// Cria a partir do valor em permil, trazendo valores fora da faixa para o limite mais próximo.
    pub fn clamped_from_permille(permille: u16) -> Self {
        Self(permille.clamp(Self::MIN.0, Self::MAX.0))
    }

    /// Cria a partir de uma porcentagem de 1 a 100.
    pub fn from_percent(percent: u8) -> Result<Self, DomainError> {
        ensure_range("porcentagem de brilho", percent.into(), 1, 100)?;
        Ok(Self(percent_to_permille(percent)))
    }

    pub fn permille(self) -> u16 {
        self.0
    }

    /// Porcentagem arredondada (meio para cima), de 1 a 100.
    pub fn percent(self) -> u8 {
        permille_to_percent(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_limits_of_the_range() {
        assert_eq!(Brightness::try_from_permille(10).unwrap().permille(), 10);
        assert_eq!(
            Brightness::try_from_permille(1000).unwrap().permille(),
            1000
        );
    }

    #[test]
    fn rejects_values_outside_the_range() {
        assert_eq!(
            Brightness::try_from_permille(9),
            Err(DomainError::OutOfRange {
                quantity: "brilho",
                value: 9,
                min: 10,
                max: 1000
            })
        );
        assert!(Brightness::try_from_permille(1001).is_err());
    }

    #[test]
    fn clamps_values_outside_the_range_to_the_nearest_limit() {
        assert_eq!(Brightness::clamped_from_permille(0), Brightness::MIN);
        assert_eq!(Brightness::clamped_from_permille(5000), Brightness::MAX);
        assert_eq!(Brightness::clamped_from_permille(500).permille(), 500);
    }

    #[test]
    fn is_created_from_a_percentage() {
        assert_eq!(Brightness::from_percent(1), Ok(Brightness::MIN));
        assert_eq!(Brightness::from_percent(100), Ok(Brightness::MAX));
        assert_eq!(Brightness::from_percent(65).unwrap().permille(), 650);
    }

    #[test]
    fn rejects_percentages_outside_1_to_100() {
        assert!(Brightness::from_percent(0).is_err());
        assert!(Brightness::from_percent(101).is_err());
    }

    #[test]
    fn rounds_the_percentage_half_up() {
        let percent = |permille| Brightness::try_from_permille(permille).unwrap().percent();

        assert_eq!(percent(10), 1);
        assert_eq!(percent(14), 1);
        assert_eq!(percent(15), 2);
        assert_eq!(percent(655), 66);
        assert_eq!(percent(1000), 100);
    }

    #[test]
    fn percentage_round_trips_for_every_value() {
        for percent in 1..=100 {
            assert_eq!(
                Brightness::from_percent(percent).unwrap().percent(),
                percent
            );
        }
    }
}
