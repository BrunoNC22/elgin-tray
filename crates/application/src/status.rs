//! Situação da conexão e o que é mostrado ao usuário.

use elgin_domain::BulbState;

/// Situação da conexão com a lâmpada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionStatus {
    /// Não há lâmpada configurada.
    NotConfigured,
    /// Tentando conectar.
    Connecting,
    /// Conectado e com o estado lido.
    Online,
    /// Sem conexão (a lâmpada não responde).
    Offline,
    /// A lâmpada recusou a local key.
    KeyRejected,
    /// A chave não pôde ser recuperada sozinha: é preciso refazer a configuração.
    NeedsReconfiguration,
}

/// O que a interface mostra: a conexão e o último estado conhecido da lâmpada.
///
/// O estado continua presente quando a lâmpada fica offline (mostrado "apagado" pela interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BulbSnapshot {
    pub connection: ConnectionStatus,
    pub state: Option<BulbState>,
}
