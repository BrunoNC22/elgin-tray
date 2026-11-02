//! Controlador da lâmpada: executa comandos com atualização otimista e mantém o estado mostrado.

use std::fmt;
use std::time::{Duration, Instant};

use elgin_domain::{BulbCommand, BulbState};

use crate::ports::{BulbGateway, BulbObserver, Clock, ControllerEvent, GatewayError};
use crate::status::{BulbSnapshot, ConnectionStatus};

/// Intervalo mínimo padrão entre dois envios para a lâmpada (ao arrastar slider ou disco de cores).
pub const DEFAULT_MIN_SEND_INTERVAL: Duration = Duration::from_millis(100);

/// Por que um comando não foi executado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// Não há conexão com a lâmpada (ou o estado dela ainda não foi lido).
    NotConnected,
    /// A lâmpada não aceitou o envio.
    Gateway(GatewayError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "a lâmpada não está conectada"),
            Self::Gateway(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ControlError {}

/// Executa comandos na lâmpada e mantém o estado mostrado ao usuário.
///
/// - **Atualização otimista:** o estado esperado é publicado antes do envio; se o envio falhar,
///   volta ao último estado confirmado e publica a falha.
/// - **Coalescência:** no máximo um envio por `min_send_interval`. Comandos que chegam antes disso
///   só atualizam o estado pendente ("o último vence"), enviado por [`flush`](Self::flush) quando
///   chegar a hora ([`next_flush_at`](Self::next_flush_at)).
/// - **Pushes:** mudanças vindas da própria lâmpada (outro app, interruptor) entram por
///   [`on_reported_state`](Self::on_reported_state).
///
/// A conexão é gerenciada por fora (supervisor): ele entrega a conexão com
/// [`attach`](Self::attach) e a retira com [`detach`](Self::detach).
#[derive(Debug)]
pub struct BulbController<G, O, C> {
    gateway: Option<G>,
    observer: O,
    clock: C,
    min_send_interval: Duration,
    connection: ConnectionStatus,
    /// Último estado confirmado pela lâmpada (lido ou enviado com sucesso).
    confirmed: Option<BulbState>,
    /// Estado mostrado ao usuário (pode estar à frente do confirmado).
    displayed: Option<BulbState>,
    /// Estado desejado que ainda espera a vez de ser enviado.
    pending: Option<BulbState>,
    last_sent_at: Option<Instant>,
}

impl<G, O, C> BulbController<G, O, C>
where
    G: BulbGateway,
    O: BulbObserver,
    C: Clock,
{
    pub fn new(observer: O, clock: C, min_send_interval: Duration) -> Self {
        Self {
            gateway: None,
            observer,
            clock,
            min_send_interval,
            connection: ConnectionStatus::NotConfigured,
            confirmed: None,
            displayed: None,
            pending: None,
            last_sent_at: None,
        }
    }

    /// O que está sendo mostrado agora.
    pub fn snapshot(&self) -> BulbSnapshot {
        BulbSnapshot {
            connection: self.connection,
            state: self.displayed,
        }
    }

    /// Muda a situação da conexão (ex.: `Connecting`, `NotConfigured`) e publica.
    pub fn set_connection(&mut self, connection: ConnectionStatus) {
        self.connection = connection;
        self.publish_state();
    }

    /// Recebe uma conexão nova e lê o estado da lâmpada. Em sucesso fica `Online`.
    pub async fn attach(&mut self, mut gateway: G) -> Result<(), GatewayError> {
        let state = gateway.fetch_state().await?;
        self.gateway = Some(gateway);
        self.pending = None;
        self.connection = ConnectionStatus::Online;
        self.confirm(state);
        Ok(())
    }

    /// Larga a conexão (caiu ou foi recusada). Mantém o último estado conhecido para mostrar,
    /// descarta o envio pendente e publica a nova situação.
    pub fn detach(&mut self, connection: ConnectionStatus) {
        self.gateway = None;
        self.pending = None;
        self.displayed = self.confirmed;
        self.set_connection(connection);
    }

    /// Lê de novo o estado da lâmpada.
    pub async fn refresh(&mut self) -> Result<(), GatewayError> {
        let gateway = self.gateway.as_mut().ok_or(GatewayError::Disconnected)?;
        let state = gateway.fetch_state().await?;
        self.pending = None;
        self.confirm(state);
        Ok(())
    }

    /// Executa um comando: publica o estado esperado e envia agora ou na próxima janela.
    pub async fn execute(&mut self, command: BulbCommand) -> Result<(), ControlError> {
        let current = match (&self.gateway, self.displayed) {
            (Some(_), Some(state)) => state,
            _ => return Err(ControlError::NotConnected),
        };
        let expected = current.apply(&command);
        self.displayed = Some(expected);
        self.publish_state();

        if self.is_send_due() {
            self.send(expected).await
        } else {
            self.pending = Some(expected);
            Ok(())
        }
    }

    /// Quando o estado pendente deve ser enviado (`None` se não há nada pendente).
    pub fn next_flush_at(&self) -> Option<Instant> {
        self.pending?;
        Some(self.last_sent_at? + self.min_send_interval)
    }

    /// Envia o estado pendente, se já chegou a hora. Antes disso não faz nada.
    pub async fn flush(&mut self) -> Result<(), ControlError> {
        match self.pending {
            Some(target) if self.is_send_due() => {
                self.pending = None;
                self.send(target).await
            }
            _ => Ok(()),
        }
    }

    /// A lâmpada informou um estado novo (push).
    ///
    /// Se há um envio pendente (usuário ainda arrastando), o estado mostrado continua sendo o
    /// pedido pelo usuário; o push só atualiza o confirmado.
    pub fn on_reported_state(&mut self, state: BulbState) {
        if self.pending.is_some() {
            self.confirmed = Some(state);
        } else {
            self.confirm(state);
        }
    }

    fn is_send_due(&self) -> bool {
        self.last_sent_at
            .is_none_or(|sent| self.clock.now() >= sent + self.min_send_interval)
    }

    async fn send(&mut self, target: BulbState) -> Result<(), ControlError> {
        let gateway = self.gateway.as_mut().ok_or(ControlError::NotConnected)?;
        self.last_sent_at = Some(self.clock.now());
        match gateway.apply(&target).await {
            Ok(()) => {
                self.confirmed = Some(target);
                Ok(())
            }
            Err(error) => {
                self.pending = None;
                self.displayed = self.confirmed;
                self.publish_state();
                self.observer
                    .notify(&ControllerEvent::CommandFailed(error.clone()));
                Err(ControlError::Gateway(error))
            }
        }
    }

    /// Guarda `state` como confirmado e mostrado, e publica.
    fn confirm(&mut self, state: BulbState) {
        self.confirmed = Some(state);
        self.displayed = Some(state);
        self.publish_state();
    }

    fn publish_state(&mut self) {
        let snapshot = self.snapshot();
        self.observer
            .notify(&ControllerEvent::StateChanged(snapshot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeBulbGateway, ManualClock, SpyObserver};
    use elgin_domain::{Brightness, ColorTemperature, LightMode, Power};

    type Controller = BulbController<FakeBulbGateway, SpyObserver, ManualClock>;

    const INTERVAL: Duration = DEFAULT_MIN_SEND_INTERVAL;

    fn white(brightness_percent: u8) -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::White {
                brightness: Brightness::from_percent(brightness_percent).unwrap(),
                temperature: ColorTemperature::WARMEST,
            },
        }
    }

    fn set_brightness(percent: u8) -> BulbCommand {
        BulbCommand::SetBrightness(Brightness::from_percent(percent).unwrap())
    }

    struct Fixture {
        controller: Controller,
        bulb: FakeBulbGateway,
        observer: SpyObserver,
        clock: ManualClock,
    }

    /// Controlador já conectado a uma lâmpada branca a 50%, com os eventos da conexão limpos.
    async fn connected() -> Fixture {
        let bulb = FakeBulbGateway::new(white(50));
        let observer = SpyObserver::new();
        let clock = ManualClock::new();
        let mut controller = Controller::new(observer.clone(), clock.clone(), INTERVAL);
        controller.attach(bulb.clone()).await.unwrap();
        observer.clear();
        Fixture {
            controller,
            bulb,
            observer,
            clock,
        }
    }

    fn online(state: BulbState) -> ControllerEvent {
        ControllerEvent::StateChanged(BulbSnapshot {
            connection: ConnectionStatus::Online,
            state: Some(state),
        })
    }

    #[test]
    fn starts_without_state_and_not_configured() {
        let controller = Controller::new(SpyObserver::new(), ManualClock::new(), INTERVAL);

        assert_eq!(
            controller.snapshot(),
            BulbSnapshot {
                connection: ConnectionStatus::NotConfigured,
                state: None
            }
        );
    }

    #[test]
    fn set_connection_publishes_the_new_status() {
        let observer = SpyObserver::new();
        let mut controller = Controller::new(observer.clone(), ManualClock::new(), INTERVAL);

        controller.set_connection(ConnectionStatus::Connecting);

        assert_eq!(
            observer.events(),
            [ControllerEvent::StateChanged(BulbSnapshot {
                connection: ConnectionStatus::Connecting,
                state: None
            })]
        );
    }

    #[tokio::test]
    async fn attach_reads_the_bulb_and_goes_online() {
        let bulb = FakeBulbGateway::new(white(50));
        let observer = SpyObserver::new();
        let mut controller = Controller::new(observer.clone(), ManualClock::new(), INTERVAL);

        controller.attach(bulb.clone()).await.unwrap();

        assert_eq!(bulb.fetch_count(), 1);
        assert_eq!(observer.events(), [online(white(50))]);
    }

    #[tokio::test]
    async fn attach_fails_when_the_state_cannot_be_read() {
        let bulb = FakeBulbGateway::new(white(50));
        bulb.fail_next(GatewayError::KeyRejected);
        let observer = SpyObserver::new();
        let mut controller = Controller::new(observer.clone(), ManualClock::new(), INTERVAL);

        let result = controller.attach(bulb).await;

        assert_eq!(result, Err(GatewayError::KeyRejected));
        assert_eq!(
            controller.snapshot().connection,
            ConnectionStatus::NotConfigured
        );
        assert!(observer.events().is_empty());
    }

    #[tokio::test]
    async fn rejects_commands_when_not_connected() {
        let observer = SpyObserver::new();
        let mut controller = Controller::new(observer.clone(), ManualClock::new(), INTERVAL);

        let result = controller.execute(BulbCommand::TogglePower).await;

        assert_eq!(result, Err(ControlError::NotConnected));
        assert!(observer.events().is_empty());
    }

    #[tokio::test]
    async fn executes_optimistically_and_sends_the_expected_state() {
        let mut f = connected().await;

        f.controller.execute(set_brightness(80)).await.unwrap();

        assert_eq!(f.observer.events(), [online(white(80))]);
        assert_eq!(f.bulb.applied(), [white(80)]);
    }

    #[tokio::test]
    async fn reverts_to_the_confirmed_state_when_sending_fails() {
        let mut f = connected().await;
        f.bulb.fail_next(GatewayError::Timeout);

        let result = f.controller.execute(set_brightness(80)).await;

        assert_eq!(result, Err(ControlError::Gateway(GatewayError::Timeout)));
        assert_eq!(
            f.observer.events(),
            [
                online(white(80)),
                online(white(50)),
                ControllerEvent::CommandFailed(GatewayError::Timeout)
            ]
        );
        assert_eq!(f.controller.snapshot().state, Some(white(50)));
    }

    #[tokio::test]
    async fn coalesces_rapid_commands_keeping_the_last_value() {
        let mut f = connected().await;

        for percent in 11..=20 {
            f.controller.execute(set_brightness(percent)).await.unwrap();
        }

        assert_eq!(f.bulb.applied(), [white(11)], "só o primeiro sai na hora");
        assert_eq!(f.controller.snapshot().state, Some(white(20)));
        assert_eq!(f.controller.next_flush_at(), Some(f.clock.now() + INTERVAL));

        f.clock.advance(INTERVAL);
        f.controller.flush().await.unwrap();

        assert_eq!(f.bulb.applied(), [white(11), white(20)]);
        assert_eq!(f.controller.next_flush_at(), None);
    }

    #[tokio::test]
    async fn flush_before_the_deadline_does_nothing() {
        let mut f = connected().await;
        f.controller.execute(set_brightness(30)).await.unwrap();
        f.controller.execute(set_brightness(40)).await.unwrap();

        f.clock.advance(INTERVAL - Duration::from_millis(1));
        f.controller.flush().await.unwrap();

        assert_eq!(f.bulb.applied(), [white(30)]);
        assert!(f.controller.next_flush_at().is_some());
    }

    #[tokio::test]
    async fn flush_without_pending_state_does_nothing() {
        let mut f = connected().await;

        f.controller.flush().await.unwrap();

        assert!(f.bulb.applied().is_empty());
        assert!(f.observer.events().is_empty());
    }

    #[tokio::test]
    async fn a_command_after_the_interval_is_sent_immediately() {
        let mut f = connected().await;
        f.controller.execute(set_brightness(30)).await.unwrap();

        f.clock.advance(INTERVAL);
        f.controller.execute(set_brightness(40)).await.unwrap();

        assert_eq!(f.bulb.applied(), [white(30), white(40)]);
        assert_eq!(f.controller.next_flush_at(), None);
    }

    #[tokio::test]
    async fn a_failed_flush_reverts_to_the_confirmed_state() {
        let mut f = connected().await;
        f.controller.execute(set_brightness(30)).await.unwrap();
        f.controller.execute(set_brightness(40)).await.unwrap();
        f.bulb.fail_next(GatewayError::Disconnected);

        f.clock.advance(INTERVAL);
        let result = f.controller.flush().await;

        assert_eq!(
            result,
            Err(ControlError::Gateway(GatewayError::Disconnected))
        );
        assert_eq!(f.controller.snapshot().state, Some(white(30)));
        assert_eq!(f.controller.next_flush_at(), None);
        assert_eq!(
            f.observer.events().last(),
            Some(&ControllerEvent::CommandFailed(GatewayError::Disconnected))
        );
    }

    #[tokio::test]
    async fn pushes_from_the_bulb_update_the_displayed_state() {
        let mut f = connected().await;

        f.controller.on_reported_state(white(5));

        assert_eq!(f.observer.events(), [online(white(5))]);
    }

    #[tokio::test]
    async fn a_push_while_a_command_is_pending_keeps_the_user_target() {
        let mut f = connected().await;
        f.controller.execute(set_brightness(30)).await.unwrap();
        f.controller.execute(set_brightness(40)).await.unwrap();
        f.observer.clear();

        f.controller.on_reported_state(white(30));

        assert_eq!(f.controller.snapshot().state, Some(white(40)));
        assert!(f.observer.events().is_empty(), "nada muda na tela");

        f.clock.advance(INTERVAL);
        f.controller.flush().await.unwrap();
        assert_eq!(f.bulb.applied().last(), Some(&white(40)));
    }

    #[tokio::test]
    async fn refresh_reads_the_bulb_again() {
        let mut f = connected().await;
        f.bulb.set_state(white(75));

        f.controller.refresh().await.unwrap();

        assert_eq!(f.observer.events(), [online(white(75))]);
    }

    #[tokio::test]
    async fn detach_keeps_the_last_state_and_drops_the_pending_command() {
        let mut f = connected().await;
        f.controller.execute(set_brightness(30)).await.unwrap();
        f.controller.execute(set_brightness(40)).await.unwrap();
        f.observer.clear();

        f.controller.detach(ConnectionStatus::Offline);

        let offline = BulbSnapshot {
            connection: ConnectionStatus::Offline,
            state: Some(white(30)),
        };
        assert_eq!(f.controller.snapshot(), offline);
        assert_eq!(f.controller.next_flush_at(), None);
        assert_eq!(
            f.observer.events(),
            [ControllerEvent::StateChanged(offline)]
        );
        assert_eq!(
            f.controller.execute(set_brightness(60)).await,
            Err(ControlError::NotConnected)
        );
    }
}
