use std::fmt;

/// Erro ao construir um valor do domínio com dados inválidos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// Um número ficou fora da faixa aceita pela grandeza.
    OutOfRange {
        quantity: &'static str,
        value: u32,
        min: u32,
        max: u32,
    },
    /// Um texto obrigatório veio vazio (ou só com espaços).
    Empty { quantity: &'static str },
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange {
                quantity,
                value,
                min,
                max,
            } => write!(
                f,
                "{quantity} fora da faixa: {value} (esperado de {min} a {max})"
            ),
            Self::Empty { quantity } => write!(f, "{quantity} não pode ser vazio"),
        }
    }
}

impl std::error::Error for DomainError {}

/// Confere se `value` está em `min..=max`; senão devolve [`DomainError::OutOfRange`].
pub(crate) fn ensure_range(
    quantity: &'static str,
    value: u32,
    min: u32,
    max: u32,
) -> Result<(), DomainError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(DomainError::OutOfRange {
            quantity,
            value,
            min,
            max,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_message_names_the_quantity_and_the_range() {
        let error = DomainError::OutOfRange {
            quantity: "brilho",
            value: 5,
            min: 10,
            max: 1000,
        };

        assert_eq!(
            error.to_string(),
            "brilho fora da faixa: 5 (esperado de 10 a 1000)"
        );
    }

    #[test]
    fn empty_message_names_the_quantity() {
        let error = DomainError::Empty {
            quantity: "nome da lâmpada",
        };

        assert_eq!(error.to_string(), "nome da lâmpada não pode ser vazio");
    }

    #[test]
    fn ensure_range_accepts_the_limits_and_rejects_outside() {
        assert_eq!(ensure_range("x", 10, 10, 20), Ok(()));
        assert_eq!(ensure_range("x", 20, 10, 20), Ok(()));
        assert!(ensure_range("x", 9, 10, 20).is_err());
        assert!(ensure_range("x", 21, 10, 20).is_err());
    }
}
