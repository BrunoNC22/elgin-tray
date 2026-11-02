//! Conversões entre permil (como a lâmpada informa) e porcentagem (como a interface mostra).

/// Converte permil (0–1000) em porcentagem arredondada meio para cima (0–100).
pub(crate) fn permille_to_percent(permille: u16) -> u8 {
    // No máximo (1000 + 5) / 10 = 100, então sempre cabe em u8.
    u8::try_from((permille.min(1000) + 5) / 10).unwrap_or(100)
}

/// Converte porcentagem (0–100) em permil.
pub(crate) fn percent_to_permille(percent: u8) -> u16 {
    u16::from(percent.min(100)) * 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permille_to_percent_rounds_half_up() {
        assert_eq!(permille_to_percent(0), 0);
        assert_eq!(permille_to_percent(4), 0);
        assert_eq!(permille_to_percent(5), 1);
        assert_eq!(permille_to_percent(994), 99);
        assert_eq!(permille_to_percent(995), 100);
        assert_eq!(permille_to_percent(1000), 100);
    }

    #[test]
    fn values_above_the_range_are_limited() {
        assert_eq!(permille_to_percent(u16::MAX), 100);
        assert_eq!(percent_to_permille(u8::MAX), 1000);
    }

    #[test]
    fn percent_to_permille_multiplies_by_ten() {
        assert_eq!(percent_to_permille(0), 0);
        assert_eq!(percent_to_permille(37), 370);
        assert_eq!(percent_to_permille(100), 1000);
    }
}
