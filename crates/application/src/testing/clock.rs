use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::ports::Clock;

/// Relógio que só anda quando o teste manda (`advance`).
#[derive(Debug, Clone)]
pub struct ManualClock {
    now: Rc<Cell<Instant>>,
}

impl ManualClock {
    pub fn new() -> Self {
        Self {
            now: Rc::new(Cell::new(Instant::now())),
        }
    }

    pub fn advance(&self, duration: Duration) {
        self.now.set(self.now.get() + duration);
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.now.get()
    }
}
