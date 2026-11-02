use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use elgin_domain::BulbState;

use crate::ports::{BulbGateway, GatewayError};

/// Lâmpada em memória: guarda o estado, registra cada `apply` (spy) e devolve falhas roteirizadas.
#[derive(Debug, Clone)]
pub struct FakeBulbGateway {
    inner: Rc<RefCell<Inner>>,
}

#[derive(Debug)]
struct Inner {
    state: BulbState,
    applied: Vec<BulbState>,
    fetches: usize,
    failures: VecDeque<GatewayError>,
}

impl FakeBulbGateway {
    pub fn new(state: BulbState) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                state,
                applied: Vec::new(),
                fetches: 0,
                failures: VecDeque::new(),
            })),
        }
    }

    /// A próxima chamada (`fetch_state` ou `apply`) falha com `error`. Pode ser chamada várias
    /// vezes para enfileirar falhas.
    pub fn fail_next(&self, error: GatewayError) {
        self.inner.borrow_mut().failures.push_back(error);
    }

    /// Estados enviados com `apply`, em ordem (inclusive os que falharam não entram).
    pub fn applied(&self) -> Vec<BulbState> {
        self.inner.borrow().applied.clone()
    }

    pub fn fetch_count(&self) -> usize {
        self.inner.borrow().fetches
    }

    /// Estado atual da lâmpada falsa.
    pub fn state(&self) -> BulbState {
        self.inner.borrow().state
    }

    /// Muda o estado "por fora" (como se outro app tivesse mexido na lâmpada).
    pub fn set_state(&self, state: BulbState) {
        self.inner.borrow_mut().state = state;
    }

    fn take_failure(&self) -> Result<(), GatewayError> {
        match self.inner.borrow_mut().failures.pop_front() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl BulbGateway for FakeBulbGateway {
    async fn fetch_state(&mut self) -> Result<BulbState, GatewayError> {
        self.inner.borrow_mut().fetches += 1;
        self.take_failure()?;
        Ok(self.inner.borrow().state)
    }

    async fn apply(&mut self, target: &BulbState) -> Result<(), GatewayError> {
        self.take_failure()?;
        let mut inner = self.inner.borrow_mut();
        inner.state = *target;
        inner.applied.push(*target);
        Ok(())
    }
}
