use std::fmt;

use crate::config::AppConfig;

/// Onde a configuração é guardada (no app real, o `config.toml`).
pub trait ConfigRepository {
    /// Lê a configuração salva; `None` se ainda não existe.
    fn load(&self) -> Result<Option<AppConfig>, ConfigError>;

    fn save(&mut self, config: &AppConfig) -> Result<(), ConfigError>;
}

/// Falha ao ler ou salvar a configuração.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Erro de leitura/escrita (permissão, disco).
    Io(String),
    /// O arquivo existe, mas é inválido.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(detail) => {
                write!(f, "não foi possível ler ou salvar a configuração: {detail}")
            }
            Self::Invalid(detail) => write!(f, "configuração inválida: {detail}"),
        }
    }
}

impl std::error::Error for ConfigError {}
