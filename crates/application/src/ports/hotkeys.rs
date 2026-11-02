use crate::hotkeys::{HotkeyAction, HotkeyBinding};

/// Registra atalhos globais no sistema. Quando um atalho é apertado, o adaptador entrega a
/// [`HotkeyAction`] ao daemon (como, fica a cargo de quem monta o adaptador).
pub trait HotkeyRegistrar {
    /// Troca os atalhos registrados por estes. Devolve os que não puderam ser registrados, com o
    /// motivo (ex.: já usado por outro programa).
    fn register(
        &mut self,
        bindings: &[(HotkeyAction, HotkeyBinding)],
    ) -> Vec<(HotkeyAction, String)>;
}
