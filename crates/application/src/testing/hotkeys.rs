use std::cell::RefCell;
use std::rc::Rc;

use crate::hotkeys::{HotkeyAction, HotkeyBinding};
use crate::ports::HotkeyRegistrar;

/// Registro de atalhos em memória: guarda o que foi registrado e recusa os atalhos marcados.
#[derive(Debug, Clone, Default)]
pub struct SpyHotkeys {
    inner: Rc<RefCell<HotkeysInner>>,
}

#[derive(Debug, Default)]
struct HotkeysInner {
    registered: Vec<(HotkeyAction, HotkeyBinding)>,
    rejected: Vec<(HotkeyBinding, String)>,
}

impl SpyHotkeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// Os atalhos registrados com sucesso no último `register`.
    pub fn registered(&self) -> Vec<(HotkeyAction, HotkeyBinding)> {
        self.inner.borrow().registered.clone()
    }

    /// Faz `binding` falhar com `reason` (como se outro programa já o usasse).
    pub fn reject(&self, binding: HotkeyBinding, reason: &str) {
        self.inner
            .borrow_mut()
            .rejected
            .push((binding, reason.to_owned()));
    }
}

impl HotkeyRegistrar for SpyHotkeys {
    fn register(
        &mut self,
        bindings: &[(HotkeyAction, HotkeyBinding)],
    ) -> Vec<(HotkeyAction, String)> {
        let mut inner = self.inner.borrow_mut();
        let mut failures = Vec::new();
        let mut registered = Vec::new();
        for &(action, binding) in bindings {
            match inner
                .rejected
                .iter()
                .find(|(rejected, _)| *rejected == binding)
            {
                Some((_, reason)) => failures.push((action, reason.clone())),
                None => registered.push((action, binding)),
            }
        }
        inner.registered = registered;
        failures
    }
}
