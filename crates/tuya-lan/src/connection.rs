//! Conexão aberta com a lâmpada: negociação + um ator que é dono do socket.
//!
//! O ator (uma tarefa local do tokio) lê o socket o tempo todo, manda heartbeats, atende um pedido
//! por vez e repassa os pushes da lâmpada como eventos. O [`TuyaGateway`] e o [`TuyaEvents`] são só
//! as pontas dos canais até ele. Quando o gateway é largado, o ator termina e fecha o socket.
//!
//! Precisa rodar dentro de um `tokio::task::LocalSet` (o daemon usa um runtime de uma thread só).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use elgin_application::ports::{BulbEvent, BulbEvents, BulbGateway, GatewayError};
use elgin_domain::BulbState;
use serde_json::{Map, Value};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep_until, timeout};

use crate::dps::{DpMap, KnownDps};
use crate::frame::{self, Frame, FrameDecoder, FrameError, Key, command};
use crate::message::{self, DeviceMessage};
use crate::random::Randomness;
use crate::session::{Handshake, HandshakeError};
use crate::transport::Transport;

/// Tempos da conexão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// Espera máxima pela resposta da negociação.
    pub handshake_timeout: Duration,
    /// Espera máxima pela resposta de um pedido (consulta ou comando).
    pub response_timeout: Duration,
    /// Intervalo entre heartbeats.
    pub heartbeat_interval: Duration,
    /// Sem receber nada por este tempo, a conexão é dada como morta.
    pub dead_after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(5),
            response_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(10),
            dead_after: Duration::from_secs(25),
        }
    }
}

/// Ponta de pedidos da conexão (implementa a porta `BulbGateway`).
#[derive(Debug)]
pub struct TuyaGateway {
    requests: mpsc::Sender<Request>,
}

/// Ponta de eventos da conexão (implementa a porta `BulbEvents`).
#[derive(Debug)]
pub struct TuyaEvents {
    events: mpsc::UnboundedReceiver<BulbEvent>,
}

#[derive(Debug)]
enum Request {
    Fetch(oneshot::Sender<Result<BulbState, GatewayError>>),
    Apply(BulbState, oneshot::Sender<Result<(), GatewayError>>),
}

impl BulbGateway for TuyaGateway {
    async fn fetch_state(&mut self) -> Result<BulbState, GatewayError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(Request::Fetch(reply))
            .await
            .map_err(|_| GatewayError::Disconnected)?;
        answer.await.unwrap_or(Err(GatewayError::Disconnected))
    }

    async fn apply(&mut self, target: &BulbState) -> Result<(), GatewayError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(Request::Apply(*target, reply))
            .await
            .map_err(|_| GatewayError::Disconnected)?;
        answer.await.unwrap_or(Err(GatewayError::Disconnected))
    }
}

impl BulbEvents for TuyaEvents {
    async fn next_event(&mut self) -> BulbEvent {
        self.events
            .recv()
            .await
            .unwrap_or(BulbEvent::ConnectionLost(GatewayError::Disconnected))
    }
}

/// Negocia a sessão sobre `transport` e inicia o ator. Precisa de um `LocalSet`.
pub(crate) async fn open<T, R>(
    mut transport: T,
    device_id: &str,
    local_key: Key,
    mut random: R,
    dp_map: DpMap,
    timing: Timing,
) -> Result<(TuyaGateway, TuyaEvents), GatewayError>
where
    T: Transport + 'static,
    R: Randomness + 'static,
{
    let mut decoder = FrameDecoder::new();
    let handshake = Handshake::new(local_key, random.nonce());

    send(&mut transport, &handshake.start_frame(1, &random.iv())).await?;
    let response = timeout(
        timing.handshake_timeout,
        read_frame(&mut transport, &mut decoder, &local_key),
    )
    .await
    .map_err(|_| GatewayError::Timeout)?
    // Com a chave errada a lâmpada fecha a conexão logo depois do pedido de negociação, sem
    // responder (confirmado na lâmpada Elgin). O tinytuya dá o mesmo diagnóstico.
    .map_err(|error| match error {
        GatewayError::Disconnected => GatewayError::KeyRejected,
        other => other,
    })?;
    let negotiated = handshake.respond(&response).map_err(handshake_error)?;
    send(
        &mut transport,
        &handshake.finish_frame(&negotiated, 2, &random.iv()),
    )
    .await?;

    let (requests, request_receiver) = mpsc::channel(8);
    let (event_sender, events) = mpsc::unbounded_channel();
    let actor = Actor {
        transport,
        random,
        decoder,
        session_key: negotiated.session_key,
        seqno: 3,
        device_id: device_id.to_owned(),
        dp_map,
        known: KnownDps::new(),
        timing,
        requests: request_receiver,
        events: event_sender,
        pending: None,
        last_received: Instant::now(),
    };
    tokio::task::spawn_local(actor.run());
    Ok((TuyaGateway { requests }, TuyaEvents { events }))
}

/// Lê até ter um quadro completo (usado só na negociação, antes do ator existir).
async fn read_frame<T: Transport>(
    transport: &mut T,
    decoder: &mut FrameDecoder,
    key: &Key,
) -> Result<Frame, GatewayError> {
    let mut buffer = [0u8; 1024];
    loop {
        match decoder.next_frame(key) {
            Ok(Some(frame)) => return Ok(frame),
            Ok(None) => {}
            Err(error) => return Err(handshake_error(HandshakeError::from(error))),
        }
        match transport.receive(&mut buffer).await {
            Ok(0) | Err(_) => return Err(GatewayError::Disconnected),
            Ok(count) => decoder.push(&buffer[..count]),
        }
    }
}

async fn send<T: Transport>(transport: &mut T, bytes: &[u8]) -> Result<(), GatewayError> {
    transport
        .send(bytes)
        .await
        .map_err(|_| GatewayError::Disconnected)
}

fn handshake_error(error: HandshakeError) -> GatewayError {
    match error {
        HandshakeError::KeyRejected => GatewayError::KeyRejected,
        other => GatewayError::Protocol(other.to_string()),
    }
}

/// O pedido que espera resposta da lâmpada (um por vez).
#[derive(Debug)]
enum Pending {
    Fetch {
        reply: oneshot::Sender<Result<BulbState, GatewayError>>,
        deadline: Instant,
    },
    Apply {
        sent: Map<String, Value>,
        reply: oneshot::Sender<Result<(), GatewayError>>,
        deadline: Instant,
    },
}

impl Pending {
    fn deadline(&self) -> Instant {
        match self {
            Self::Fetch { deadline, .. } | Self::Apply { deadline, .. } => *deadline,
        }
    }

    fn fail(self, error: GatewayError) {
        // Quem pediu pode já ter desistido; não há o que fazer nesse caso.
        match self {
            Self::Fetch { reply, .. } => drop(reply.send(Err(error))),
            Self::Apply { reply, .. } => drop(reply.send(Err(error))),
        }
    }
}

/// Dono do socket depois da negociação.
struct Actor<T, R> {
    transport: T,
    random: R,
    decoder: FrameDecoder,
    session_key: Key,
    seqno: u32,
    device_id: String,
    dp_map: DpMap,
    known: KnownDps,
    timing: Timing,
    requests: mpsc::Receiver<Request>,
    events: mpsc::UnboundedSender<BulbEvent>,
    pending: Option<Pending>,
    last_received: Instant,
}

impl<T: Transport, R: Randomness> Actor<T, R> {
    async fn run(mut self) {
        let mut heartbeat = interval_at(
            Instant::now() + self.timing.heartbeat_interval,
            self.timing.heartbeat_interval,
        );
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut buffer = [0u8; 1024];

        let error = loop {
            let deadline = self.pending.as_ref().map(Pending::deadline);
            let step = tokio::select! {
                request = self.requests.recv(), if self.pending.is_none() => match request {
                    Some(request) => self.start(request).await,
                    // O gateway foi largado: fecha a conexão sem avisar nada.
                    None => return,
                },
                read = self.transport.receive(&mut buffer) => match read {
                    Ok(0) | Err(_) => Err(GatewayError::Disconnected),
                    Ok(count) => {
                        let bytes = buffer[..count].to_vec();
                        self.on_bytes(&bytes)
                    }
                },
                _ = heartbeat.tick() => self.heartbeat().await,
                () = wait_until(deadline) => Err(GatewayError::Timeout),
            };
            if let Err(error) = step {
                break error;
            }
        };

        if let Some(pending) = self.pending.take() {
            pending.fail(error.clone());
        }
        // Se ninguém escuta mais os eventos, não há a quem avisar.
        let _ = self.events.send(BulbEvent::ConnectionLost(error));
    }

    async fn start(&mut self, request: Request) -> Result<(), GatewayError> {
        let deadline = Instant::now() + self.timing.response_timeout;
        match request {
            Request::Fetch(reply) => {
                self.send(command::DP_QUERY_NEW, &message::query_payload())
                    .await?;
                self.pending = Some(Pending::Fetch { reply, deadline });
            }
            Request::Apply(target, reply) => {
                let dps = self.known.dps_for(&target, &self.dp_map);
                if dps.is_empty() {
                    let _ = reply.send(Ok(()));
                    return Ok(());
                }
                let payload = message::control_payload(&dps, unix_time());
                self.send(command::CONTROL_NEW, &payload).await?;
                self.pending = Some(Pending::Apply {
                    sent: dps,
                    reply,
                    deadline,
                });
            }
        }
        Ok(())
    }

    fn on_bytes(&mut self, bytes: &[u8]) -> Result<(), GatewayError> {
        self.last_received = Instant::now();
        self.decoder.push(bytes);
        loop {
            match self.decoder.next_frame(&self.session_key) {
                Ok(Some(frame)) => self.on_frame(&frame),
                Ok(None) => return Ok(()),
                // Um quadro corrompido é descartado; a conexão segue.
                Err(
                    FrameError::Authentication | FrameError::BadSuffix | FrameError::BadLength(_),
                ) => {}
                Err(FrameError::BadPrefix) => {}
            }
        }
    }

    fn on_frame(&mut self, frame: &Frame) {
        let Some(message) = DeviceMessage::parse(&frame.plaintext) else {
            return;
        };
        match frame.command {
            command::STATUS => {
                if let Some(dps) = message.dps() {
                    self.known.merge(&dps);
                    if let Ok(state) = self.known.to_state(&self.dp_map) {
                        let _ = self.events.send(BulbEvent::StateReported(state));
                    }
                }
            }
            command::DP_QUERY_NEW => {
                if let Some(dps) = message.dps() {
                    self.known.merge(&dps);
                }
                if let Some(Pending::Fetch { reply, .. }) = self.take_pending_if(is_fetch) {
                    let state = self
                        .known
                        .to_state(&self.dp_map)
                        .map_err(|error| GatewayError::Protocol(error.to_string()));
                    let _ = reply.send(state);
                }
            }
            command::CONTROL_NEW => {
                if let Some(Pending::Apply { sent, reply, .. }) = self.take_pending_if(is_apply) {
                    let result = if message.retcode == 0 {
                        self.known.merge(&sent);
                        Ok(())
                    } else {
                        Err(GatewayError::Protocol(format!(
                            "a lâmpada recusou o comando (código {})",
                            message.retcode
                        )))
                    };
                    let _ = reply.send(result);
                }
            }
            // Heartbeat e o resto: só contam como "a lâmpada está viva" (já registrado).
            _ => {}
        }
    }

    fn take_pending_if(&mut self, matches: fn(&Pending) -> bool) -> Option<Pending> {
        if self.pending.as_ref().is_some_and(matches) {
            self.pending.take()
        } else {
            None
        }
    }

    async fn heartbeat(&mut self) -> Result<(), GatewayError> {
        if self.last_received.elapsed() >= self.timing.dead_after {
            return Err(GatewayError::Timeout);
        }
        let payload = message::heartbeat_payload(&self.device_id);
        self.send(command::HEART_BEAT, &payload).await
    }

    async fn send(&mut self, command: u32, payload: &[u8]) -> Result<(), GatewayError> {
        let bytes = frame::encode(
            self.seqno,
            command,
            payload,
            &self.session_key,
            &self.random.iv(),
        );
        self.seqno = self.seqno.wrapping_add(1);
        send(&mut self.transport, &bytes).await
    }
}

fn is_fetch(pending: &Pending) -> bool {
    matches!(pending, Pending::Fetch { .. })
}

fn is_apply(pending: &Pending) -> bool {
    matches!(pending, Pending::Apply { .. })
}

/// Espera até `deadline`; sem prazo, espera para sempre.
async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_lamp::{Behaviour, FakeLamp};
    use crate::random::OsRandomness;
    use elgin_domain::{Brightness, ColorTemperature, Hsv, Hue, LightMode, Power, Saturation};
    use serde_json::json;
    use std::future::Future;

    const KEY: Key = *b"0123456789abcdef";

    fn real_status() -> Value {
        json!({"20": true, "21": "white", "22": 1000, "23": 1000, "24": "000003e803e8", "26": 0})
    }

    fn white(brightness: u16, temperature: u16) -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::White {
                brightness: Brightness::try_from_permille(brightness).unwrap(),
                temperature: ColorTemperature::try_from_permille(temperature).unwrap(),
            },
        }
    }

    /// Roda dentro de um `LocalSet` (o ator usa `spawn_local`).
    async fn local<F: Future>(future: F) -> F::Output {
        tokio::task::LocalSet::new().run_until(future).await
    }

    async fn connect(lamp: &FakeLamp, key: Key) -> Result<(TuyaGateway, TuyaEvents), GatewayError> {
        open(
            lamp.clone(),
            "lampada-teste",
            key,
            OsRandomness,
            DpMap::default(),
            Timing::default(),
        )
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn negotiates_and_reads_the_state() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());

            let (mut gateway, _events) = connect(&lamp, KEY).await.unwrap();
            let state = gateway.fetch_state().await;

            assert_eq!(state, Ok(white(1000, 1000)));
            assert_eq!(
                lamp.received(),
                [
                    command::SESS_KEY_NEG_START,
                    command::SESS_KEY_NEG_FINISH,
                    command::DP_QUERY_NEW
                ]
            );
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_wrong_key_is_rejected() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());

            let result = connect(&lamp, *b"chave-errada-123").await;

            assert_eq!(result.err(), Some(GatewayError::KeyRejected));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_wrong_key_answered_with_another_key_is_rejected_too() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            lamp.set_behaviour(Behaviour {
                close_on_wrong_key: false,
                ..Behaviour::default()
            });

            let result = connect(&lamp, *b"chave-errada-123").await;

            assert_eq!(result.err(), Some(GatewayError::KeyRejected));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_lamp_times_out_during_the_handshake() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            lamp.set_behaviour(Behaviour {
                answer_handshake: false,
                ..Behaviour::default()
            });

            let result = connect(&lamp, KEY).await;

            assert_eq!(result.err(), Some(GatewayError::Timeout));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_lamp_that_closes_during_the_handshake_is_disconnected() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            lamp.close();

            let result = connect(&lamp, KEY).await;

            assert_eq!(result.err(), Some(GatewayError::Disconnected));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn apply_sends_the_changed_dps_and_waits_for_the_confirmation() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            let (mut gateway, _events) = connect(&lamp, KEY).await.unwrap();
            gateway.fetch_state().await.unwrap();

            let result = gateway.apply(&white(250, 1000)).await;

            assert_eq!(result, Ok(()));
            assert_eq!(
                lamp.controls(),
                [json!({"22": 250, "23": 1000}).as_object().unwrap().clone()]
            );
            assert_eq!(lamp.dps()["22"], json!(250));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn applying_a_colour_switches_the_mode() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            let (mut gateway, _events) = connect(&lamp, KEY).await.unwrap();
            gateway.fetch_state().await.unwrap();
            let blue = BulbState {
                power: Power::On,
                mode: LightMode::Colour(Hsv::new(Hue::new(240), Saturation::FULL, Brightness::MAX)),
            };

            gateway.apply(&blue).await.unwrap();

            assert_eq!(
                lamp.controls()[0],
                json!({"21": "colour", "24": "00f003e803e8"})
                    .as_object()
                    .unwrap()
                    .clone()
            );
            assert_eq!(gateway.fetch_state().await, Ok(blue));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn applying_the_current_state_sends_nothing() {
        local(async {
            let lamp = FakeLamp::new(KEY, json!({"20": false, "21": "scene"}));
            let (mut gateway, _events) = connect(&lamp, KEY).await.unwrap();
            gateway.fetch_state().await.unwrap();
            let off_scene = BulbState {
                power: Power::Off,
                mode: LightMode::Scene,
            };

            assert_eq!(gateway.apply(&off_scene).await, Ok(()));
            assert!(lamp.controls().is_empty());
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_command_is_a_protocol_error() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            lamp.set_behaviour(Behaviour {
                control_retcode: 1,
                ..Behaviour::default()
            });
            let (mut gateway, _events) = connect(&lamp, KEY).await.unwrap();
            gateway.fetch_state().await.unwrap();

            let result = gateway.apply(&white(250, 1000)).await;

            assert!(
                matches!(result, Err(GatewayError::Protocol(_))),
                "{result:?}"
            );
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn pushes_from_the_lamp_become_events() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            let (mut gateway, mut events) = connect(&lamp, KEY).await.unwrap();
            gateway.fetch_state().await.unwrap();

            lamp.push(json!({"22": 500}));

            assert_eq!(
                events.next_event().await,
                BulbEvent::StateReported(white(500, 1000))
            );
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn sends_heartbeats_periodically() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            let (_gateway, _events) = connect(&lamp, KEY).await.unwrap();

            tokio::time::sleep(Timing::default().heartbeat_interval * 2 + Duration::from_millis(1))
                .await;

            let heartbeats = lamp
                .received()
                .iter()
                .filter(|&&cmd| cmd == command::HEART_BEAT)
                .count();
            assert_eq!(heartbeats, 2);
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_lamp_that_stops_answering_is_reported_lost() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            lamp.set_behaviour(Behaviour {
                answer_heartbeats: false,
                ..Behaviour::default()
            });
            let (_gateway, mut events) = connect(&lamp, KEY).await.unwrap();

            assert_eq!(
                events.next_event().await,
                BulbEvent::ConnectionLost(GatewayError::Timeout)
            );
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_connection_is_reported_lost_and_requests_fail() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            let (mut gateway, mut events) = connect(&lamp, KEY).await.unwrap();

            lamp.close();

            assert_eq!(
                events.next_event().await,
                BulbEvent::ConnectionLost(GatewayError::Disconnected)
            );
            assert_eq!(gateway.fetch_state().await, Err(GatewayError::Disconnected));
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn an_unanswered_request_times_out_and_loses_the_connection() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            lamp.set_behaviour(Behaviour {
                answer_queries: false,
                ..Behaviour::default()
            });
            let (mut gateway, mut events) = connect(&lamp, KEY).await.unwrap();

            assert_eq!(gateway.fetch_state().await, Err(GatewayError::Timeout));
            assert_eq!(
                events.next_event().await,
                BulbEvent::ConnectionLost(GatewayError::Timeout)
            );
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_gateway_closes_the_connection() {
        local(async {
            let lamp = FakeLamp::new(KEY, real_status());
            let (gateway, _events) = connect(&lamp, KEY).await.unwrap();
            assert_eq!(lamp.handles(), 2, "o teste e o ator");

            drop(gateway);
            tokio::task::yield_now().await;

            assert_eq!(lamp.handles(), 1, "o ator terminou e largou a lâmpada");
        })
        .await;
    }
}
