use crate::ports::GatewayError;
use crate::status::BulbSnapshot;

/// O que o controlador avisa para quem mostra o estado (bandeja, painel, ícone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerEvent {
    /// O estado mostrado mudou (comando otimista, push da lâmpada, conexão).
    StateChanged(BulbSnapshot),
    /// Um comando não chegou à lâmpada; o estado já voltou ao último confirmado.
    CommandFailed(GatewayError),
}

/// Recebe os eventos do controlador.
pub trait BulbObserver {
    fn notify(&mut self, event: &ControllerEvent);
}
