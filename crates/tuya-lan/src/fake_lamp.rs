//! Lâmpada falsa para testes: implementa [`Transport`] falando o protocolo 3.5 de verdade.
//!
//! Decifra o que o cliente manda, negocia a chave de sessão, responde consultas, comandos e
//! heartbeats, e pode mandar pushes ou fechar a conexão quando o teste pedir. O teste guarda um
//! clone para roteirizar e inspecionar.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use hmac::{Hmac, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use tokio::sync::Notify;

use crate::frame::{self, FrameDecoder, Key, command};
use crate::message::{DeviceMessage, VERSION_HEADER};
use crate::random::Nonce;
use crate::session::derive_session_key;
use crate::transport::Transport;

const REMOTE_NONCE: Nonce = [0x5a; 16];

/// Como a lâmpada falsa se comporta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Behaviour {
    pub(crate) answer_handshake: bool,
    pub(crate) answer_queries: bool,
    pub(crate) answer_controls: bool,
    pub(crate) answer_heartbeats: bool,
    /// Retcode das confirmações de comando (0 = aceito).
    pub(crate) control_retcode: u32,
    /// Com a chave errada, fechar a conexão (como a lâmpada Elgin real faz) em vez de responder com
    /// um quadro cifrado com a chave dela.
    pub(crate) close_on_wrong_key: bool,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            answer_handshake: true,
            answer_queries: true,
            answer_controls: true,
            answer_heartbeats: true,
            control_retcode: 0,
            close_on_wrong_key: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FakeLamp {
    inner: Rc<RefCell<Inner>>,
    readable: Rc<Notify>,
}

#[derive(Debug)]
struct Inner {
    key: Key,
    behaviour: Behaviour,
    dps: Map<String, Value>,
    local_nonce: Option<Nonce>,
    session_key: Option<Key>,
    decoder: FrameDecoder,
    outgoing: Vec<u8>,
    closed: bool,
    /// Comandos recebidos do cliente, em ordem.
    received: Vec<u32>,
    /// DPs de cada comando `CONTROL_NEW` recebido.
    controls: Vec<Map<String, Value>>,
    seqno: u32,
    iv_counter: u8,
}

impl FakeLamp {
    /// Lâmpada com a chave `key` e os DPs iniciais `dps`.
    pub(crate) fn new(key: Key, dps: Value) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                key,
                behaviour: Behaviour::default(),
                dps: dps.as_object().cloned().unwrap_or_default(),
                local_nonce: None,
                session_key: None,
                decoder: FrameDecoder::new(),
                outgoing: Vec::new(),
                closed: false,
                received: Vec::new(),
                controls: Vec::new(),
                seqno: 100,
                iv_counter: 0,
            })),
            readable: Rc::new(Notify::new()),
        }
    }

    pub(crate) fn set_behaviour(&self, behaviour: Behaviour) {
        self.inner.borrow_mut().behaviour = behaviour;
    }

    /// Comandos recebidos do cliente, em ordem.
    pub(crate) fn received(&self) -> Vec<u32> {
        self.inner.borrow().received.clone()
    }

    /// DPs dos comandos recebidos.
    pub(crate) fn controls(&self) -> Vec<Map<String, Value>> {
        self.inner.borrow().controls.clone()
    }

    pub(crate) fn dps(&self) -> Map<String, Value> {
        self.inner.borrow().dps.clone()
    }

    /// Quantos clones existem (o teste usa para ver se a conexão largou a lâmpada).
    pub(crate) fn handles(&self) -> usize {
        Rc::strong_count(&self.inner)
    }

    /// A lâmpada muda sozinha (outro app, interruptor) e avisa com um push.
    pub(crate) fn push(&self, changes: Value) {
        let changes = changes.as_object().cloned().unwrap_or_default();
        let mut inner = self.inner.borrow_mut();
        inner.dps.extend(changes.clone());
        inner.send_push(&changes);
        drop(inner);
        self.readable.notify_one();
    }

    /// A lâmpada fecha a conexão.
    pub(crate) fn close(&self) {
        self.inner.borrow_mut().closed = true;
        self.readable.notify_one();
    }
}

impl Inner {
    fn reply(&mut self, cmd: u32, retcode: u32, body: &[u8], key: Key) {
        let plaintext = [retcode.to_be_bytes().as_slice(), body].concat();
        self.iv_counter = self.iv_counter.wrapping_add(1);
        let iv = [self.iv_counter; 12];
        self.seqno += 1;
        let bytes = frame::encode(self.seqno, cmd, &plaintext, &key, &iv);
        self.outgoing.extend_from_slice(&bytes);
    }

    fn send_push(&mut self, changes: &Map<String, Value>) {
        let Some(session_key) = self.session_key else {
            return;
        };
        let json = json!({"protocol": 4, "t": 1, "data": {"dps": changes}}).to_string();
        let body = [VERSION_HEADER.as_slice(), json.as_bytes()].concat();
        self.reply(command::STATUS, 0, &body, session_key);
    }

    fn handle(&mut self, bytes: &[u8]) {
        self.decoder.push(bytes);
        loop {
            // Antes da sessão os quadros usam a chave da lâmpada; depois, a de sessão.
            let key = self.session_key.unwrap_or(self.key);
            let frame = match self.decoder.next_frame(&key) {
                Ok(Some(frame)) => frame,
                Ok(None) => return,
                // Cliente com outra chave: a lâmpada real fecha a conexão. A alternativa responde com
                // a chave dela, como se fosse um pedido de negociação (o cliente falha na autenticação).
                Err(_) => {
                    if self.behaviour.close_on_wrong_key {
                        self.closed = true;
                        return;
                    }
                    if self.behaviour.answer_handshake {
                        let proof = [0u8; 32];
                        let body = [REMOTE_NONCE.as_slice(), &proof].concat();
                        self.reply(command::SESS_KEY_NEG_RESP, 0, &body, self.key);
                    }
                    continue;
                }
            };
            self.received.push(frame.command);
            match frame.command {
                command::SESS_KEY_NEG_START if self.behaviour.answer_handshake => {
                    let local_nonce: Nonce = frame.plaintext[..16].try_into().unwrap();
                    self.local_nonce = Some(local_nonce);
                    let proof = <Hmac<Sha256> as Mac>::new_from_slice(&self.key)
                        .unwrap()
                        .chain_update(local_nonce)
                        .finalize()
                        .into_bytes();
                    let body = [REMOTE_NONCE.as_slice(), &proof].concat();
                    self.reply(command::SESS_KEY_NEG_RESP, 0, &body, self.key);
                }
                command::SESS_KEY_NEG_FINISH => {
                    let local_nonce = self.local_nonce.expect("negociação sem início");
                    self.session_key =
                        Some(derive_session_key(&self.key, &local_nonce, &REMOTE_NONCE));
                }
                command::DP_QUERY_NEW if self.behaviour.answer_queries => {
                    let body = json!({"dps": self.dps}).to_string();
                    let key = self.session_key.unwrap();
                    self.reply(command::DP_QUERY_NEW, 0, body.as_bytes(), key);
                }
                command::CONTROL_NEW if self.behaviour.answer_controls => {
                    // O cliente manda como a lâmpada recebe: sem retcode, com cabeçalho de versão.
                    let with_fake_retcode = [[0u8; 4].as_slice(), &frame.plaintext].concat();
                    let changes = DeviceMessage::parse(&with_fake_retcode)
                        .and_then(|message| message.dps())
                        .unwrap_or_default();
                    self.controls.push(changes.clone());
                    let key = self.session_key.unwrap();
                    let retcode = self.behaviour.control_retcode;
                    self.reply(command::CONTROL_NEW, retcode, &[], key);
                    if retcode == 0 {
                        self.dps.extend(changes.clone());
                        self.send_push(&changes);
                    }
                }
                command::HEART_BEAT if self.behaviour.answer_heartbeats => {
                    let key = self.session_key.unwrap();
                    self.reply(command::HEART_BEAT, 0, &[], key);
                }
                _ => {}
            }
        }
    }
}

impl Transport for FakeLamp {
    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.inner.borrow().closed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.inner.borrow_mut().handle(bytes);
        self.readable.notify_one();
        Ok(())
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            {
                let mut inner = self.inner.borrow_mut();
                if !inner.outgoing.is_empty() {
                    let count = inner.outgoing.len().min(buffer.len());
                    buffer[..count].copy_from_slice(&inner.outgoing[..count]);
                    inner.outgoing.drain(..count);
                    return Ok(count);
                }
                if inner.closed {
                    return Ok(0);
                }
            }
            self.readable.notified().await;
        }
    }
}
