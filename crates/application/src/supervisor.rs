//! Supervisor de conexão: conecta, reconecta com espera crescente e reencontra a lâmpada se o IP mudar.

use std::time::{Duration, Instant};

use elgin_domain::BulbId;

use crate::controller::BulbController;
use crate::credentials::{Endpoint, LocalKey};
use crate::ports::{BulbConnector, BulbDiscovery, BulbObserver, Clock, GatewayError};
use crate::status::ConnectionStatus;

/// Quando e como tentar de novo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Espera depois da primeira falha; dobra a cada falha seguida.
    pub initial_delay: Duration,
    /// Teto da espera.
    pub max_delay: Duration,
    /// A cada quantas falhas seguidas procurar a lâmpada na rede (o IP pode ter mudado).
    pub discovery_every: u32,
    /// Quanto esperar pelo anúncio da lâmpada numa busca.
    pub discovery_timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            discovery_every: 3,
            discovery_timeout: Duration::from_secs(5),
        }
    }
}

/// Qual lâmpada conectar e com qual chave. Sem `endpoint`, a lâmpada é procurada na rede antes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionTarget {
    pub id: BulbId,
    pub key: LocalKey,
    pub endpoint: Option<Endpoint>,
}

/// Algo que quem usa o supervisor precisa tratar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorEvent {
    /// A lâmpada foi encontrada num endereço novo: salvar na configuração.
    EndpointChanged(Endpoint),
    /// A lâmpada recusou a chave: as tentativas pararam até [`ConnectionSupervisor::update_target`]
    /// (fluxo de recuperação da chave).
    KeyRejected,
}

/// Mantém a conexão com a lâmpada viva e entrega cada conexão nova ao controlador.
///
/// Não tem loop próprio: o daemon chama [`tick`](Self::tick) no horário de
/// [`next_attempt_at`](Self::next_attempt_at) e avisa quedas com
/// [`connection_lost`](Self::connection_lost).
#[derive(Debug)]
pub struct ConnectionSupervisor<K, D, C> {
    connector: K,
    discovery: D,
    clock: C,
    policy: RetryPolicy,
    target: ConnectionTarget,
    /// Falhas seguidas desde a última conexão bem-sucedida.
    failures: u32,
    next_attempt_at: Option<Instant>,
}

impl<K, D, C> ConnectionSupervisor<K, D, C>
where
    K: BulbConnector,
    D: BulbDiscovery,
    C: Clock,
{
    /// Cria o supervisor com a primeira tentativa marcada para agora.
    pub fn new(
        connector: K,
        discovery: D,
        clock: C,
        policy: RetryPolicy,
        target: ConnectionTarget,
    ) -> Self {
        let now = clock.now();
        Self {
            connector,
            discovery,
            clock,
            policy,
            target,
            failures: 0,
            next_attempt_at: Some(now),
        }
    }

    pub fn target(&self) -> &ConnectionTarget {
        &self.target
    }

    /// Quando tentar conectar (`None`: conectado, ou parado por chave recusada).
    pub fn next_attempt_at(&self) -> Option<Instant> {
        self.next_attempt_at
    }

    /// Troca a lâmpada/chave/endereço (ex.: chave nova recuperada) e marca uma tentativa para agora.
    pub fn update_target(&mut self, target: ConnectionTarget) {
        self.target = target;
        self.failures = 0;
        self.next_attempt_at = Some(self.clock.now());
    }

    /// Tenta conectar, se já chegou a hora.
    pub async fn tick<O, CC>(
        &mut self,
        controller: &mut BulbController<K::Gateway, O, CC>,
    ) -> Vec<SupervisorEvent>
    where
        O: BulbObserver,
        CC: Clock,
    {
        let mut events = Vec::new();
        match self.next_attempt_at {
            Some(at) if self.clock.now() >= at => {}
            _ => return events,
        }
        if self.failures == 0 {
            controller.set_connection(ConnectionStatus::Connecting);
        }

        let endpoint = match self.target.endpoint.clone() {
            Some(endpoint) => endpoint,
            None => match self.discover().await {
                Some(found) => {
                    self.target.endpoint = Some(found.clone());
                    events.push(SupervisorEvent::EndpointChanged(found.clone()));
                    found
                }
                None => {
                    events.extend(
                        self.fail(controller, GatewayError::Unreachable, false)
                            .await,
                    );
                    return events;
                }
            },
        };

        let connected = match self
            .connector
            .connect(&self.target.id, &self.target.key, &endpoint)
            .await
        {
            Ok(gateway) => controller.attach(gateway).await,
            Err(error) => Err(error),
        };
        match connected {
            Ok(()) => {
                self.failures = 0;
                self.next_attempt_at = None;
            }
            Err(error) => events.extend(self.fail(controller, error, true).await),
        }
        events
    }

    /// A conexão que estava com o controlador caiu (detectado pelo daemon).
    pub async fn connection_lost<O, CC>(
        &mut self,
        controller: &mut BulbController<K::Gateway, O, CC>,
        error: GatewayError,
    ) -> Vec<SupervisorEvent>
    where
        O: BulbObserver,
        CC: Clock,
    {
        self.failures = 0;
        self.fail(controller, error, true).await
    }

    /// Registra uma falha: chave recusada para tudo; o resto fica offline e marca a próxima
    /// tentativa (antes, a cada `discovery_every` falhas, procura um endereço novo, se `may_discover`).
    async fn fail<O, CC>(
        &mut self,
        controller: &mut BulbController<K::Gateway, O, CC>,
        error: GatewayError,
        may_discover: bool,
    ) -> Vec<SupervisorEvent>
    where
        O: BulbObserver,
        CC: Clock,
    {
        if error == GatewayError::KeyRejected {
            self.failures = 0;
            self.next_attempt_at = None;
            controller.detach(ConnectionStatus::KeyRejected);
            return vec![SupervisorEvent::KeyRejected];
        }

        self.failures += 1;
        controller.detach(ConnectionStatus::Offline);

        if may_discover
            && self.failures.is_multiple_of(self.policy.discovery_every)
            && let Some(found) = self.discover().await
            && self.target.endpoint.as_ref() != Some(&found)
        {
            self.target.endpoint = Some(found.clone());
            self.next_attempt_at = Some(self.clock.now());
            return vec![SupervisorEvent::EndpointChanged(found)];
        }

        self.next_attempt_at = Some(self.clock.now() + self.backoff());
        Vec::new()
    }

    async fn discover(&mut self) -> Option<Endpoint> {
        self.discovery
            .find(&self.target.id, self.policy.discovery_timeout)
            .await
    }

    /// `initial_delay` dobrando a cada falha seguida, até `max_delay`.
    fn backoff(&self) -> Duration {
        let doublings = self.failures.saturating_sub(1).min(31);
        self.policy
            .initial_delay
            .saturating_mul(1 << doublings)
            .min(self.policy.max_delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::DEFAULT_MIN_SEND_INTERVAL;
    use crate::ports::ControllerEvent;
    use crate::status::BulbSnapshot;
    use crate::testing::{FakeBulbGateway, ManualClock, SpyObserver, StubConnector, StubDiscovery};
    use elgin_domain::{BulbState, LightMode, Power};
    use std::net::{IpAddr, Ipv4Addr};

    type Controller = BulbController<FakeBulbGateway, SpyObserver, ManualClock>;
    type Supervisor = ConnectionSupervisor<StubConnector, StubDiscovery, ManualClock>;

    const SECOND: Duration = Duration::from_secs(1);

    fn endpoint(last_octet: u8) -> Endpoint {
        Endpoint {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 0, last_octet)),
            protocol_version: "3.5".to_owned(),
        }
    }

    fn target(endpoint: Option<Endpoint>) -> ConnectionTarget {
        ConnectionTarget {
            id: BulbId::try_new("lampada-teste").unwrap(),
            key: LocalKey::new("chave-falsa-1234"),
            endpoint,
        }
    }

    fn bulb() -> FakeBulbGateway {
        FakeBulbGateway::new(BulbState {
            power: Power::On,
            mode: LightMode::Scene,
        })
    }

    struct Fixture {
        supervisor: Supervisor,
        controller: Controller,
        connector: StubConnector,
        discovery: StubDiscovery,
        observer: SpyObserver,
        clock: ManualClock,
    }

    fn fixture(endpoint: Option<Endpoint>) -> Fixture {
        let connector = StubConnector::new();
        let discovery = StubDiscovery::new();
        let observer = SpyObserver::new();
        let clock = ManualClock::new();
        let supervisor = Supervisor::new(
            connector.clone(),
            discovery.clone(),
            clock.clone(),
            RetryPolicy::default(),
            target(endpoint),
        );
        let controller =
            Controller::new(observer.clone(), clock.clone(), DEFAULT_MIN_SEND_INTERVAL);
        Fixture {
            supervisor,
            controller,
            connector,
            discovery,
            observer,
            clock,
        }
    }

    impl Fixture {
        async fn tick(&mut self) -> Vec<SupervisorEvent> {
            self.supervisor.tick(&mut self.controller).await
        }

        fn connection(&self) -> ConnectionStatus {
            self.controller.snapshot().connection
        }

        /// Avança o relógio até a próxima tentativa marcada e devolve quanto esperou.
        fn wait_for_next_attempt(&self) -> Duration {
            let at = self
                .supervisor
                .next_attempt_at()
                .expect("sem tentativa marcada");
            let wait = at.saturating_duration_since(self.clock.now());
            self.clock.advance(wait);
            wait
        }
    }

    #[tokio::test]
    async fn connects_on_the_first_tick_and_goes_online() {
        let mut f = fixture(Some(endpoint(10)));
        f.connector.succeed_with(bulb());

        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now()));
        let events = f.tick().await;

        assert!(events.is_empty());
        assert_eq!(f.connection(), ConnectionStatus::Online);
        assert_eq!(f.supervisor.next_attempt_at(), None);
        assert_eq!(f.connector.attempts()[0].endpoint, endpoint(10));
        assert_eq!(
            f.connector.attempts()[0].key,
            LocalKey::new("chave-falsa-1234")
        );
    }

    #[tokio::test]
    async fn shows_connecting_before_the_first_attempt() {
        let mut f = fixture(Some(endpoint(10)));
        f.connector.succeed_with(bulb());

        f.tick().await;

        assert_eq!(
            f.observer.events()[0],
            ControllerEvent::StateChanged(BulbSnapshot {
                connection: ConnectionStatus::Connecting,
                state: None
            })
        );
    }

    #[tokio::test]
    async fn does_nothing_before_the_next_attempt() {
        let mut f = fixture(Some(endpoint(10)));
        f.tick().await; // falha (nada roteirizado = Unreachable)

        f.clock.advance(SECOND - Duration::from_millis(1));
        f.tick().await;

        assert_eq!(f.connector.attempts().len(), 1);
    }

    #[tokio::test]
    async fn a_failure_goes_offline_and_retries_after_one_second() {
        let mut f = fixture(Some(endpoint(10)));

        f.tick().await;

        assert_eq!(f.connection(), ConnectionStatus::Offline);
        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now() + SECOND));
    }

    #[tokio::test]
    async fn the_wait_doubles_after_each_failure_up_to_one_minute() {
        // Descoberta sempre acha o mesmo IP, então só o backoff conta.
        let mut f = fixture(Some(endpoint(10)));
        for _ in 0..10 {
            f.discovery.answer(Some(endpoint(10)));
        }

        let mut waits = Vec::new();
        f.tick().await;
        for _ in 0..8 {
            waits.push(f.wait_for_next_attempt().as_secs());
            f.tick().await;
        }

        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
    }

    #[tokio::test]
    async fn after_three_failures_finds_the_new_address_and_retries_at_once() {
        let mut f = fixture(Some(endpoint(10)));
        f.discovery.answer(Some(endpoint(77)));

        f.tick().await;
        f.wait_for_next_attempt();
        f.tick().await;
        f.wait_for_next_attempt();
        let events = f.tick().await;

        assert_eq!(f.discovery.searches().len(), 1);
        assert_eq!(
            f.discovery.searches()[0].1,
            RetryPolicy::default().discovery_timeout
        );
        assert_eq!(events, [SupervisorEvent::EndpointChanged(endpoint(77))]);
        assert_eq!(f.supervisor.target().endpoint, Some(endpoint(77)));
        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now()));

        f.connector.succeed_with(bulb());
        f.tick().await;
        assert_eq!(
            f.connector.attempts().last().unwrap().endpoint,
            endpoint(77)
        );
        assert_eq!(f.connection(), ConnectionStatus::Online);
    }

    #[tokio::test]
    async fn the_same_address_found_again_keeps_the_normal_wait() {
        let mut f = fixture(Some(endpoint(10)));
        f.discovery.answer(Some(endpoint(10)));

        f.tick().await;
        f.wait_for_next_attempt();
        f.tick().await;
        f.wait_for_next_attempt();
        let events = f.tick().await;

        assert!(events.is_empty());
        assert_eq!(
            f.supervisor.next_attempt_at(),
            Some(f.clock.now() + 4 * SECOND)
        );
    }

    #[tokio::test]
    async fn without_a_known_address_searches_the_network_first() {
        let mut f = fixture(None);
        f.discovery.answer(Some(endpoint(55)));
        f.connector.succeed_with(bulb());

        let events = f.tick().await;

        assert_eq!(events, [SupervisorEvent::EndpointChanged(endpoint(55))]);
        assert_eq!(f.connector.attempts()[0].endpoint, endpoint(55));
        assert_eq!(f.connection(), ConnectionStatus::Online);
    }

    #[tokio::test]
    async fn without_a_known_address_and_not_found_it_waits_and_tries_again() {
        let mut f = fixture(None);

        let events = f.tick().await;

        assert!(events.is_empty());
        assert!(f.connector.attempts().is_empty());
        assert_eq!(f.connection(), ConnectionStatus::Offline);
        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now() + SECOND));
    }

    #[tokio::test]
    async fn a_rejected_key_stops_the_attempts() {
        let mut f = fixture(Some(endpoint(10)));
        f.connector.fail_with(GatewayError::KeyRejected);

        let events = f.tick().await;

        assert_eq!(events, [SupervisorEvent::KeyRejected]);
        assert_eq!(f.connection(), ConnectionStatus::KeyRejected);
        assert_eq!(f.supervisor.next_attempt_at(), None);
    }

    #[tokio::test]
    async fn a_new_target_restarts_the_attempts_immediately() {
        let mut f = fixture(Some(endpoint(10)));
        f.connector.fail_with(GatewayError::KeyRejected);
        f.tick().await;

        let mut new_target = target(Some(endpoint(10)));
        new_target.key = LocalKey::new("chave-nova-56789");
        f.supervisor.update_target(new_target);
        f.connector.succeed_with(bulb());
        f.tick().await;

        assert_eq!(
            f.connector.attempts()[1].key,
            LocalKey::new("chave-nova-56789")
        );
        assert_eq!(f.connection(), ConnectionStatus::Online);
    }

    #[tokio::test]
    async fn failing_to_read_the_state_after_connecting_counts_as_a_failure() {
        let mut f = fixture(Some(endpoint(10)));
        let unreadable = bulb();
        unreadable.fail_next(GatewayError::Timeout);
        f.connector.succeed_with(unreadable);

        f.tick().await;

        assert_eq!(f.connection(), ConnectionStatus::Offline);
        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now() + SECOND));
    }

    #[tokio::test]
    async fn a_lost_connection_goes_offline_and_retries_with_a_fresh_wait() {
        let mut f = fixture(Some(endpoint(10)));
        f.connector.succeed_with(bulb());
        f.tick().await;

        let events = f
            .supervisor
            .connection_lost(&mut f.controller, GatewayError::Disconnected)
            .await;

        assert!(events.is_empty());
        assert_eq!(f.connection(), ConnectionStatus::Offline);
        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now() + SECOND));
    }

    #[tokio::test]
    async fn a_lost_connection_by_rejected_key_stops_the_attempts() {
        let mut f = fixture(Some(endpoint(10)));
        f.connector.succeed_with(bulb());
        f.tick().await;

        let events = f
            .supervisor
            .connection_lost(&mut f.controller, GatewayError::KeyRejected)
            .await;

        assert_eq!(events, [SupervisorEvent::KeyRejected]);
        assert_eq!(f.connection(), ConnectionStatus::KeyRejected);
        assert_eq!(f.supervisor.next_attempt_at(), None);
    }

    #[tokio::test]
    async fn a_success_resets_the_wait() {
        let mut f = fixture(Some(endpoint(10)));
        f.tick().await;
        f.wait_for_next_attempt();
        f.tick().await; // 2 falhas: próxima espera seria 4 s
        f.wait_for_next_attempt();
        f.connector.succeed_with(bulb());
        f.discovery.answer(Some(endpoint(10)));
        f.tick().await;

        f.supervisor
            .connection_lost(&mut f.controller, GatewayError::Disconnected)
            .await;

        assert_eq!(f.supervisor.next_attempt_at(), Some(f.clock.now() + SECOND));
    }
}
