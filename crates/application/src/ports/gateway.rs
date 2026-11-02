use std::fmt;

use elgin_domain::BulbState;

/// Conexão aberta com uma lâmpada.
///
/// A aplicação não manda comandos para a lâmpada: manda o **estado desejado** (`apply`), e o
/// adaptador decide o que enviar (no Tuya, quais DPs). Assim, juntar vários comandos rápidos é só
/// ficar com o último estado desejado.
pub trait BulbGateway {
    /// Lê o estado atual da lâmpada.
    async fn fetch_state(&mut self) -> Result<BulbState, GatewayError>;

    /// Deixa a lâmpada no estado `target`.
    async fn apply(&mut self, target: &BulbState) -> Result<(), GatewayError>;
}

/// Falha ao falar com a lâmpada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    /// A lâmpada não respondeu na rede (desligada no interruptor, IP mudou, fora da rede).
    Unreachable,
    /// A lâmpada demorou demais para responder.
    Timeout,
    /// A lâmpada recusou a local key (ex.: foi pareada de novo).
    KeyRejected,
    /// A conexão caiu.
    Disconnected,
    /// Resposta inesperada ou inválida.
    Protocol(String),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable => write!(f, "a lâmpada não respondeu na rede"),
            Self::Timeout => write!(f, "a lâmpada demorou demais para responder"),
            Self::KeyRejected => write!(f, "a lâmpada recusou a chave local"),
            Self::Disconnected => write!(f, "a conexão com a lâmpada caiu"),
            Self::Protocol(detail) => write!(f, "resposta inválida da lâmpada: {detail}"),
        }
    }
}

impl std::error::Error for GatewayError {}
