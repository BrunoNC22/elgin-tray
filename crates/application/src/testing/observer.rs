use std::cell::RefCell;
use std::rc::Rc;

use crate::ports::{BulbObserver, ControllerEvent};
use crate::status::BulbSnapshot;

/// Observador que só grava os eventos recebidos.
#[derive(Debug, Clone, Default)]
pub struct SpyObserver {
    events: Rc<RefCell<Vec<ControllerEvent>>>,
}

impl SpyObserver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<ControllerEvent> {
        self.events.borrow().clone()
    }

    /// O último `StateChanged` recebido.
    pub fn last_snapshot(&self) -> Option<BulbSnapshot> {
        self.events
            .borrow()
            .iter()
            .rev()
            .find_map(|event| match event {
                ControllerEvent::StateChanged(snapshot) => Some(*snapshot),
                ControllerEvent::CommandFailed(_) => None,
            })
    }

    pub fn clear(&self) {
        self.events.borrow_mut().clear();
    }
}

impl BulbObserver for SpyObserver {
    fn notify(&mut self, event: &ControllerEvent) {
        self.events.borrow_mut().push(event.clone());
    }
}
