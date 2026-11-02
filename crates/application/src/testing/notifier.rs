use std::cell::RefCell;
use std::rc::Rc;

use crate::ports::Notifier;

/// Guarda as notificações (título, texto) em ordem.
#[derive(Debug, Clone, Default)]
pub struct SpyNotifier {
    sent: Rc<RefCell<Vec<(String, String)>>>,
}

impl SpyNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sent(&self) -> Vec<(String, String)> {
        self.sent.borrow().clone()
    }
}

impl Notifier for SpyNotifier {
    fn notify(&mut self, title: &str, body: &str) {
        self.sent
            .borrow_mut()
            .push((title.to_owned(), body.to_owned()));
    }
}
