use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use elgin_domain::BulbId;

use crate::credentials::{Endpoint, LocalKey};
use crate::ports::{BulbConnector, BulbDiscovery, BulbEvent, BulbEvents, GatewayError};
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

type Connection = (FakeBulbGateway, ScriptedEvents);

#[derive(Debug, Default)]
struct ConnectorInner {
    results: VecDeque<Result<Connection, GatewayError>>,
    attempts: Vec<ConnectAttempt>,
}

impl StubConnector {
    pub fn new() -> Self {
        Self::default()
    }

    /// A próxima conexão dá certo, com uma lâmpada que não manda eventos.
    pub fn succeed_with(&self, gateway: FakeBulbGateway) {
        self.succeed_with_events(gateway, ScriptedEvents::new());
    }

    /// A próxima conexão dá certo, com os eventos dados.
    pub fn succeed_with_events(&self, gateway: FakeBulbGateway, events: ScriptedEvents) {
        self.inner
            .borrow_mut()
            .results
            .push_back(Ok((gateway, events)));
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
    type Events = ScriptedEvents;

    async fn connect(
        &mut self,
        id: &BulbId,
        key: &LocalKey,
        endpoint: &Endpoint,
    ) -> Result<Connection, GatewayError> {
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

/// Eventos roteirizados: entrega os enfileirados em ordem; sem nada na fila, fica esperando para
/// sempre (como uma lâmpada quieta).
#[derive(Debug, Clone, Default)]
pub struct ScriptedEvents {
    queue: Rc<RefCell<VecDeque<BulbEvent>>>,
}

impl ScriptedEvents {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, event: BulbEvent) {
        self.queue.borrow_mut().push_back(event);
    }
}

impl BulbEvents for ScriptedEvents {
    async fn next_event(&mut self) -> BulbEvent {
        let next = self.queue.borrow_mut().pop_front();
        match next {
            Some(event) => event,
            None => std::future::pending().await,
        }
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
