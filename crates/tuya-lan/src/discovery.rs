//! Descoberta de lâmpadas pela rede local (UDP).
//!
//! Lâmpadas Tuya se anunciam por UDP: 3.1 na porta 6666 (texto), 3.3+ na 6667 (cifrado com a chave
//! UDP). Lâmpadas 3.5, como a Elgin, só respondem a um **pedido de descoberta** (cmd `0x25`)
//! mandado em broadcast para a porta 7000 — e respondem na porta 7000 (confirmado com uma captura
//! real, ver `tests/fixtures/real_broadcast.json`). Por isso a busca manda o pedido de tempos em
//! tempos enquanto escuta as três portas.
//!
//! Formatos aceitos: quadro 6699 com AES-GCM (3.5, com ou sem retcode), quadro 55AA com AES-ECB
//! (3.3) ou JSON puro dentro do 55AA (3.1).

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::time::Duration;

use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use elgin_application::credentials::Endpoint;
use elgin_application::ports::BulbDiscovery;
use elgin_domain::BulbId;
use serde_json::Value;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::time::{Instant, sleep_until};

use crate::frame::{self, Iv, Key, command};
use crate::random::{OsRandomness, Randomness};

/// Chave UDP da Tuya: md5("yGAdlopoPVldABfn"). Pública, igual para todas as lâmpadas.
pub(crate) const UDP_KEY: Key = [
    0x6c, 0x1e, 0xc8, 0xe2, 0xbb, 0x9b, 0xb5, 0x9a, 0xb5, 0x0b, 0x0d, 0xaf, 0x64, 0x9b, 0x41, 0x0a,
];

const LISTEN_PORTS: [u16; 3] = [6666, 6667, 7000];
const REQUEST_PORT: u16 = 7000;
const PREFIX_55AA: [u8; 4] = [0x00, 0x00, 0x55, 0xaa];

/// O que uma lâmpada anuncia sobre si.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Announcement {
    pub(crate) device_id: String,
    pub(crate) ip: IpAddr,
    pub(crate) version: String,
}

/// Lê um anúncio. `None` para qualquer coisa que não seja o anúncio de uma lâmpada (inclusive
/// pedidos de descoberta de apps).
pub(crate) fn decode_announcement(datagram: &[u8]) -> Option<Announcement> {
    let json = if datagram.starts_with(&frame::PREFIX) {
        let plaintext = frame::decode(datagram, &UDP_KEY).ok()?.plaintext;
        // O retcode (4 bytes) pode ou não vir antes do JSON.
        match (plaintext.first(), plaintext.get(4)) {
            (Some(b'{'), _) => plaintext,
            (_, Some(b'{')) => plaintext[4..].to_vec(),
            _ => return None,
        }
    } else if datagram.starts_with(&PREFIX_55AA) {
        let data = payload_55aa(datagram)?;
        if data.first() == Some(&b'{') {
            data.to_vec()
        } else {
            decrypt_ecb(data)?
        }
    } else {
        return None;
    };
    parse_announcement(&json)
}

/// O dado de um quadro 55AA: header (16) + retcode (4) + dado + crc (4) + sufixo (4).
fn payload_55aa(datagram: &[u8]) -> Option<&[u8]> {
    let length = u32::from_be_bytes(datagram.get(12..16)?.try_into().ok()?) as usize;
    datagram.get(16 + 4..16 + length.checked_sub(8)?)
}

/// AES-128-ECB com a chave UDP e padding PKCS#7 (formato 3.3).
fn decrypt_ecb(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(&UDP_KEY));
    let mut plain = data.to_vec();
    for block in plain.as_chunks_mut::<16>().0 {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
    let padding = usize::from(*plain.last()?);
    if !(1..=16).contains(&padding) {
        return None;
    }
    plain.truncate(plain.len() - padding);
    Some(plain)
}

fn parse_announcement(json: &[u8]) -> Option<Announcement> {
    let end = json.iter().rposition(|&byte| byte != 0)? + 1;
    let value: Value = serde_json::from_slice(&json[..end]).ok()?;
    Some(Announcement {
        device_id: value.get("gwId")?.as_str()?.to_owned(),
        ip: value.get("ip")?.as_str()?.parse().ok()?,
        version: value.get("version")?.as_str()?.to_owned(),
    })
}

/// Pedido de descoberta (cmd `0x25`), cifrado com a chave UDP.
pub(crate) fn discovery_request(my_ip: &str, iv: &Iv) -> Vec<u8> {
    let ip = Value::from(my_ip).to_string();
    let payload = format!(r#"{{"from":"app","ip":{ip}}}"#);
    frame::encode(0, command::REQ_DEVINFO, payload.as_bytes(), &UDP_KEY, iv)
}

/// Onde os datagramas passam. No app são os sockets UDP; nos testes, um dublê.
pub(crate) trait Datagrams {
    /// Manda `bytes` em broadcast para a porta do pedido de descoberta.
    async fn broadcast(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Próximo datagrama recebido em qualquer uma das portas escutadas.
    async fn receive(&mut self) -> io::Result<Vec<u8>>;
}

/// Procura a lâmpada `device_id`, mandando o pedido de descoberta a cada `request_every`, até
/// `timeout`.
pub(crate) async fn find_device<D: Datagrams, R: Randomness>(
    datagrams: &mut D,
    random: &mut R,
    my_ip: &str,
    device_id: &str,
    timeout: Duration,
    request_every: Duration,
) -> Option<Endpoint> {
    let deadline = Instant::now() + timeout;
    let mut next_request = Instant::now();
    loop {
        tokio::select! {
            () = sleep_until(deadline) => return None,
            () = sleep_until(next_request) => {
                // Falhar ao mandar um pedido não encerra a busca: pode haver anúncios espontâneos.
                let _ = datagrams.broadcast(&discovery_request(my_ip, &random.iv())).await;
                next_request += request_every;
            }
            received = datagrams.receive() => {
                // Erros de recepção (ex.: ICMP "porta inalcançável" no Windows) são ignorados.
                if let Some(announcement) = received.ok().as_deref().and_then(decode_announcement)
                    && announcement.device_id == device_id
                {
                    return Some(Endpoint {
                        ip: announcement.ip,
                        protocol_version: announcement.version,
                    });
                }
            }
        }
    }
}

/// Descoberta real pela rede local. Os sockets só ficam abertos durante cada busca.
#[derive(Debug, Clone)]
pub struct TuyaDiscovery {
    request_every: Duration,
}

impl Default for TuyaDiscovery {
    fn default() -> Self {
        Self {
            request_every: Duration::from_secs(2),
        }
    }
}

impl BulbDiscovery for TuyaDiscovery {
    async fn find(&mut self, id: &BulbId, timeout: Duration) -> Option<Endpoint> {
        let mut sockets = UdpDatagrams::bind()?;
        let my_ip = local_ip().map_or_else(|| "0.0.0.0".to_owned(), |ip| ip.to_string());
        find_device(
            &mut sockets,
            &mut OsRandomness,
            &my_ip,
            id.as_str(),
            timeout,
            self.request_every,
        )
        .await
    }
}

/// Sockets UDP de escuta (6666, 6667, 7000) e um de envio em broadcast.
struct UdpDatagrams {
    listeners: Vec<UdpSocket>,
    sender: UdpSocket,
}

impl UdpDatagrams {
    /// Abre o que der. Uma porta ocupada por outro programa é pulada; sem nenhuma, `None`.
    fn bind() -> Option<Self> {
        let listeners: Vec<UdpSocket> = LISTEN_PORTS
            .iter()
            .filter_map(|&port| udp_socket(port))
            .collect();
        if listeners.is_empty() {
            return None;
        }
        Some(Self {
            listeners,
            sender: udp_socket(0)?,
        })
    }
}

/// Socket UDP com broadcast e reuso de endereço (para conviver com outros apps Tuya na máquina).
fn udp_socket(port: u16) -> Option<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    socket.set_reuse_address(true).ok()?;
    socket.set_broadcast(true).ok()?;
    socket.set_nonblocking(true).ok()?;
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    socket.bind(&address.into()).ok()?;
    UdpSocket::from_std(StdUdpSocket::from(socket)).ok()
}

impl Datagrams for UdpDatagrams {
    async fn broadcast(&mut self, bytes: &[u8]) -> io::Result<()> {
        let target = SocketAddr::from((Ipv4Addr::BROADCAST, REQUEST_PORT));
        self.sender.send_to(bytes, target).await.map(|_| ())
    }

    async fn receive(&mut self) -> io::Result<Vec<u8>> {
        let receives = self.listeners.iter().map(|socket| {
            Box::pin(async move {
                let mut buffer = vec![0u8; 2048];
                let (count, _) = socket.recv_from(&mut buffer).await?;
                buffer.truncate(count);
                Ok::<_, io::Error>(buffer)
            })
        });
        // O primeiro socket que receber algo; os outros pedidos são cancelados sem perder dados.
        first_ready(receives.collect()).await
    }
}

/// Espera o primeiro de vários futures terminar.
async fn first_ready<F: std::future::Future + Unpin>(mut futures: Vec<F>) -> F::Output {
    std::future::poll_fn(|context| {
        for future in &mut futures {
            if let std::task::Poll::Ready(output) = std::pin::Pin::new(future).poll(context) {
                return std::task::Poll::Ready(output);
            }
        }
        std::task::Poll::Pending
    })
    .await
}

/// IP deste computador na rede local (a interface da rota padrão). Não manda nenhum pacote:
/// "conectar" um socket UDP só escolhe a interface.
fn local_ip() -> Option<IpAddr> {
    let socket = StdUdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(10, 255, 255, 255), 1)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_vectors::{hex, vectors};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const EVERY: Duration = Duration::from_secs(2);

    fn real_broadcast() -> Vec<u8> {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/real_broadcast.json")).unwrap();
        hex(fixture["datagram"].as_str().unwrap())
    }

    fn vector_broadcast(name: &str) -> Vec<u8> {
        let all = vectors();
        let broadcasts = all["udp"]["broadcasts"].as_array().unwrap();
        let broadcast = broadcasts.iter().find(|b| b["name"] == name).unwrap();
        hex(broadcast["datagram"].as_str().unwrap())
    }

    fn device_id() -> String {
        vectors()["device_id"].as_str().unwrap().to_owned()
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    /// Rede falsa: entrega datagramas roteirizados (por tempo) e registra os broadcasts.
    #[derive(Debug, Clone, Default)]
    struct FakeNetwork {
        inner: Rc<RefCell<NetworkInner>>,
    }

    #[derive(Debug, Default)]
    struct NetworkInner {
        /// (quando chega, conteúdo ou erro)
        incoming: VecDeque<(Instant, io::Result<Vec<u8>>)>,
        broadcasts: Vec<Vec<u8>>,
    }

    impl FakeNetwork {
        fn arrives_after(&self, after: Duration, datagram: io::Result<Vec<u8>>) {
            let arrival = Instant::now() + after;
            self.inner
                .borrow_mut()
                .incoming
                .push_back((arrival, datagram));
        }

        fn broadcasts(&self) -> Vec<Vec<u8>> {
            self.inner.borrow().broadcasts.clone()
        }
    }

    impl Datagrams for FakeNetwork {
        async fn broadcast(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.inner.borrow_mut().broadcasts.push(bytes.to_vec());
            Ok(())
        }

        /// Como um socket de verdade, pode ser cancelado sem perder o datagrama: só tira da fila
        /// depois que ele "chegou".
        async fn receive(&mut self) -> io::Result<Vec<u8>> {
            let arrival = self.inner.borrow().incoming.front().map(|(at, _)| *at);
            match arrival {
                Some(at) => {
                    sleep_until(at).await;
                    let (_, datagram) = self.inner.borrow_mut().incoming.pop_front().unwrap();
                    datagram
                }
                None => std::future::pending().await,
            }
        }
    }

    async fn find(network: &mut FakeNetwork, timeout: Duration) -> Option<Endpoint> {
        find_device(
            network,
            &mut OsRandomness,
            "192.168.0.10",
            &device_id(),
            timeout,
            EVERY,
        )
        .await
    }

    #[test]
    fn the_udp_key_is_the_one_used_by_tinytuya() {
        assert_eq!(
            UDP_KEY.to_vec(),
            hex(vectors()["udp"]["udp_key"].as_str().unwrap())
        );
    }

    #[test]
    fn reads_the_real_answer_of_the_elgin_bulb() {
        assert_eq!(
            decode_announcement(&real_broadcast()),
            Some(Announcement {
                device_id: "bfbfbfbfbfbfbfbfbfbfbf".to_owned(),
                ip: ip("192.168.0.50"),
                version: "3.5".to_owned(),
            })
        );
    }

    #[test]
    fn reads_3_5_announcements_with_and_without_retcode() {
        for name in ["broadcast_35_with_retcode", "broadcast_35_without_retcode"] {
            let announcement = decode_announcement(&vector_broadcast(name)).unwrap();
            assert_eq!(announcement.device_id, device_id(), "{name}");
            assert_eq!(announcement.version, "3.5", "{name}");
        }
    }

    #[test]
    fn reads_3_3_announcements() {
        let announcement = decode_announcement(&vector_broadcast("broadcast_33_55aa")).unwrap();

        assert_eq!(announcement.version, "3.3");
        assert_eq!(announcement.ip, ip("192.168.0.50"));
    }

    #[test]
    fn ignores_garbage_and_discovery_requests() {
        assert_eq!(decode_announcement(b"isto nao e um anuncio"), None);
        assert_eq!(decode_announcement(&[]), None);
        assert_eq!(
            decode_announcement(&discovery_request("192.168.0.10", &[1; 12])),
            None
        );
    }

    #[test]
    fn builds_the_discovery_request_like_tinytuya() {
        let request = &vectors()["udp"]["discovery_request"];
        let iv: Iv = hex(request["iv"].as_str().unwrap()).try_into().unwrap();

        assert_eq!(
            discovery_request("192.168.0.10", &iv),
            hex(request["datagram"].as_str().unwrap())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn finds_the_bulb_that_answers() {
        let mut network = FakeNetwork::default();
        network.arrives_after(
            Duration::from_millis(300),
            Ok(vector_broadcast("broadcast_35_with_retcode")),
        );

        let found = find(&mut network, Duration::from_secs(5)).await;

        assert_eq!(
            found,
            Some(Endpoint {
                ip: ip("192.168.0.50"),
                protocol_version: "3.5".to_owned()
            })
        );
        assert_eq!(network.broadcasts().len(), 1, "pede assim que começa");
        assert!(decode_announcement(&network.broadcasts()[0]).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn ignores_other_bulbs_and_noise() {
        let mut network = FakeNetwork::default();
        network.arrives_after(Duration::ZERO, Ok(real_broadcast())); // outra lâmpada
        network.arrives_after(Duration::ZERO, Err(io::ErrorKind::ConnectionReset.into()));
        network.arrives_after(Duration::ZERO, Ok(b"lixo".to_vec()));
        network.arrives_after(
            Duration::from_secs(1),
            Ok(vector_broadcast("broadcast_35_without_retcode")),
        );

        let found = find(&mut network, Duration::from_secs(5)).await;

        assert_eq!(found.map(|endpoint| endpoint.ip), Some(ip("192.168.0.50")));
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_at_the_timeout_asking_again_periodically() {
        let mut network = FakeNetwork::default();
        let started = Instant::now();

        let found = find(&mut network, Duration::from_secs(5)).await;

        assert_eq!(found, None);
        assert_eq!(started.elapsed(), Duration::from_secs(5));
        assert_eq!(network.broadcasts().len(), 3, "em 0 s, 2 s e 4 s");
    }
}
