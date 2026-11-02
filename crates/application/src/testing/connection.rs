use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use elgin_domain::BulbId;

use crate::credentials::{Endpoint, LocalKey};
use crate::ports::{BulbConnector, BulbDiscovery, GatewayError};
use crate::testing::FakeBulbGateway;

/// Uma chamada registrada de [`StubConnector::connect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectAttempt {
    pub id: BulbId,
    pub key: LocalKey,
    pub endpoint: Endpoint,
}

/// Conector roteirizado: devolve os resultados enfileirados em ordem e registra cada tentativa.
/// Sem nada enfileirado, responde `Unreachable`.
#[derive(Debug, Clone, Default)]
pub struct StubConnector {
    inner: Rc<RefCell<ConnectorInner>>,
}

#[derive(Debug, Default)]
struct ConnectorInner {
    results: VecDeque<Result<FakeBulbGateway, GatewayError>>,
    attempts: Vec<ConnectAttempt>,
}

impl StubConnector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn succeed_with(&self, gateway: FakeBulbGateway) {
        self.inner.borrow_mut().results.push_back(Ok(gateway));
    }

    pub fn fail_with(&self, error: GatewayError) {
        self.inner.borrow_mut().results.push_back(Err(error));
    }

    pub fn attempts(&self) -> Vec<ConnectAttempt> {
        self.inner.borrow().attempts.clone()
    }
}

impl BulbConnector for StubConnector {
    type Gateway = FakeBulbGateway;

    async fn connect(
        &mut self,
        id: &BulbId,
        key: &LocalKey,
        endpoint: &Endpoint,
    ) -> Result<FakeBulbGateway, GatewayError> {
        let mut inner = self.inner.borrow_mut();
        inner.attempts.push(ConnectAttempt {
            id: id.clone(),
            key: key.clone(),
            endpoint: endpoint.clone(),
        });
        inner
            .results
            .pop_front()
            .unwrap_or(Err(GatewayError::Unreachable))
    }
}

/// Descoberta roteirizada: devolve as respostas enfileiradas em ordem e conta as buscas.
/// Sem nada enfileirado, não acha a lâmpada (`None`).
#[derive(Debug, Clone, Default)]
pub struct StubDiscovery {
    inner: Rc<RefCell<DiscoveryInner>>,
}

#[derive(Debug, Default)]
struct DiscoveryInner {
    answers: VecDeque<Option<Endpoint>>,
    searches: Vec<(BulbId, Duration)>,
}

impl StubDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn answer(&self, endpoint: Option<Endpoint>) {
        self.inner.borrow_mut().answers.push_back(endpoint);
    }

    /// Buscas feitas: (lâmpada procurada, tempo de espera).
    pub fn searches(&self) -> Vec<(BulbId, Duration)> {
        self.inner.borrow().searches.clone()
    }
}

impl BulbDiscovery for StubDiscovery {
    async fn find(&mut self, id: &BulbId, timeout: Duration) -> Option<Endpoint> {
        let mut inner = self.inner.borrow_mut();
        inner.searches.push((id.clone(), timeout));
        inner.answers.pop_front().flatten()
    }
}
