use std::cell::RefCell;
use std::rc::Rc;

use crate::ports::AutostartRegistrar;

/// Registro de autostart em memória: grava cada pedido (spy) e pode falhar uma vez.
#[derive(Debug, Clone, Default)]
pub struct SpyAutostart {
    inner: Rc<RefCell<AutostartInner>>,
}

#[derive(Debug, Default)]
struct AutostartInner {
    calls: Vec<bool>,
    fail_next: bool,
}

impl SpyAutostart {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pedidos recebidos, em ordem (`true` = ligar).
    pub fn calls(&self) -> Vec<bool> {
        self.inner.borrow().calls.clone()
    }

    pub fn fail_next(&self) {
        self.inner.borrow_mut().fail_next = true;
    }
}

impl AutostartRegistrar for SpyAutostart {
    fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let mut inner = self.inner.borrow_mut();
        inner.calls.push(enabled);
        if std::mem::take(&mut inner.fail_next) {
            Err("falha roteirizada".to_owned())
        } else {
            Ok(())
        }
    }
}
