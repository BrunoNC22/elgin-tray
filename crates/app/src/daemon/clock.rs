//! Relógio do daemon: segue o tempo do tokio (assim os testes com tempo pausado controlam tudo).

use std::time::Instant;

use elgin_application::ports::Clock;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokioClock;

impl Clock for TokioClock {
    fn now(&self) -> Instant {
        tokio::time::Instant::now().into_std()
    }
}
