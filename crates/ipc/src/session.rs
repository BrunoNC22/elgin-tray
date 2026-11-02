//! A conversa sobre uma conexão já aberta, independente do transporte (testada com streams em memória).
//!
//! Lado do daemon: [`serve_client`] atende um painel (ou uma segunda execução do app pedindo
//! `Activate`). Lado do painel: [`PanelClient`].

use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

use crate::codec::{LineDecoder, encode};
use crate::messages::{DaemonMessage, PROTOCOL_VERSION, PanelMessage, SnapshotDto, is_compatible};

/// Quanto esperar o `Hello` de quem conectou.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// As pontas do daemon que cada conexão usa.
#[derive(Debug, Clone)]
pub struct DaemonChannels {
    /// Pedidos dos painéis (tudo menos `Hello`) para o loop do daemon.
    pub requests: mpsc::UnboundedSender<PanelMessage>,
    /// O snapshot mais recente (`None` antes do primeiro).
    pub snapshots: watch::Receiver<Option<SnapshotDto>>,
    /// Vira `true` quando os painéis devem fechar (daemon saindo ou atualizando).
    pub shutdown: watch::Receiver<bool>,
}

/// Atende uma conexão até ela fechar.
///
/// Exige `Hello` compatível primeiro (senão responde `Shutdown` e fecha); depois manda o snapshot
/// atual e cada snapshot novo, e repassa os pedidos ao daemon.
pub async fn serve_client<S>(stream: S, mut channels: DaemonChannels)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut decoder = LineDecoder::new();

    let hello = tokio::time::timeout(
        HELLO_TIMEOUT,
        read_message::<PanelMessage, _>(&mut reader, &mut decoder),
    )
    .await;
    match hello {
        Ok(Ok(Some(PanelMessage::Hello { protocol_version })))
            if is_compatible(protocol_version) => {}
        Ok(Ok(Some(PanelMessage::Hello { .. }))) => {
            let _ = write(&mut writer, &DaemonMessage::Shutdown).await;
            return;
        }
        // Sem Hello, outra mensagem, conexão fechada ou silêncio: encerra.
        _ => return,
    }

    if *channels.shutdown.borrow() {
        let _ = write(&mut writer, &DaemonMessage::Shutdown).await;
        return;
    }
    let current = channels.snapshots.borrow_and_update().clone();
    if let Some(snapshot) = current
        && write(&mut writer, &DaemonMessage::Snapshot(snapshot))
            .await
            .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            message = read_message::<PanelMessage, _>(&mut reader, &mut decoder) => match message {
                Ok(Some(PanelMessage::Hello { .. })) => {}
                Ok(Some(message)) => {
                    if channels.requests.send(message).is_err() {
                        return;
                    }
                }
                Ok(None) | Err(_) => return,
            },
            changed = channels.snapshots.changed() => {
                if changed.is_err() {
                    return;
                }
                let latest = channels.snapshots.borrow_and_update().clone();
                if let Some(snapshot) = latest
                    && write(&mut writer, &DaemonMessage::Snapshot(snapshot)).await.is_err()
                {
                    return;
                }
            }
            changed = channels.shutdown.changed() => {
                if changed.is_err() || *channels.shutdown.borrow() {
                    let _ = write(&mut writer, &DaemonMessage::Shutdown).await;
                    return;
                }
            }
        }
    }
}

/// Lê até ter uma mensagem inteira; `Ok(None)` quando a outra ponta fecha. Linhas inválidas são
/// puladas. Pode ser cancelada sem perder dados (só espera dentro do `read`).
async fn read_message<T, R>(reader: &mut R, decoder: &mut LineDecoder) -> io::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
    R: AsyncRead + Unpin,
{
    let mut buffer = [0u8; 4096];
    loop {
        match decoder.next_message::<T>() {
            Some(Ok(message)) => return Ok(Some(message)),
            Some(Err(_)) => continue,
            None => {}
        }
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(None);
        }
        decoder.push(&buffer[..count]);
    }
}

async fn write<W, T>(writer: &mut W, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
{
    writer.write_all(&encode(message)).await?;
    writer.flush().await
}

/// Ponta do painel: manda `Hello` ao conectar e troca mensagens com o daemon.
#[derive(Debug)]
pub struct PanelClient<S> {
    stream: S,
    decoder: LineDecoder,
}

impl<S: AsyncRead + AsyncWrite + Unpin> PanelClient<S> {
    /// Começa a conversa (manda `Hello`).
    pub async fn start(stream: S) -> io::Result<Self> {
        let mut client = Self {
            stream,
            decoder: LineDecoder::new(),
        };
        client
            .send(&PanelMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
            })
            .await?;
        Ok(client)
    }

    pub async fn send(&mut self, message: &PanelMessage) -> io::Result<()> {
        write(&mut self.stream, message).await
    }

    /// Próxima mensagem do daemon; `None` quando a conexão fechou.
    pub async fn receive(&mut self) -> io::Result<Option<DaemonMessage>> {
        read_message(&mut self.stream, &mut self.decoder).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{CommandDto, ConnectionDto};
    use tokio::io::{DuplexStream, duplex};

    struct Daemon {
        requests: mpsc::UnboundedReceiver<PanelMessage>,
        snapshots: watch::Sender<Option<SnapshotDto>>,
        shutdown: watch::Sender<bool>,
        served: tokio::task::JoinHandle<()>,
    }

    fn snapshot(connection: ConnectionDto) -> SnapshotDto {
        SnapshotDto {
            bulb_name: Some("Quarto".to_owned()),
            connection,
            state: None,
            favorites: Vec::new(),
            setup: None,
        }
    }

    /// Um daemon atendendo a outra ponta do stream devolvido.
    fn daemon(initial: Option<SnapshotDto>) -> (Daemon, DuplexStream) {
        let (panel_side, daemon_side) = duplex(4096);
        let (requests_tx, requests) = mpsc::unbounded_channel();
        let (snapshots, snapshots_rx) = watch::channel(initial);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let channels = DaemonChannels {
            requests: requests_tx,
            snapshots: snapshots_rx,
            shutdown: shutdown_rx,
        };
        let served = tokio::spawn(serve_client(daemon_side, channels));
        (
            Daemon {
                requests,
                snapshots,
                shutdown,
                served,
            },
            panel_side,
        )
    }

    #[tokio::test]
    async fn a_panel_gets_the_current_snapshot_after_hello() {
        let (_daemon, stream) = daemon(Some(snapshot(ConnectionDto::Online)));

        let mut panel = PanelClient::start(stream).await.unwrap();

        assert_eq!(
            panel.receive().await.unwrap(),
            Some(DaemonMessage::Snapshot(snapshot(ConnectionDto::Online)))
        );
    }

    #[tokio::test]
    async fn new_snapshots_are_delivered() {
        let (daemon, stream) = daemon(None);
        let mut panel = PanelClient::start(stream).await.unwrap();

        daemon
            .snapshots
            .send_replace(Some(snapshot(ConnectionDto::Offline)));

        assert_eq!(
            panel.receive().await.unwrap(),
            Some(DaemonMessage::Snapshot(snapshot(ConnectionDto::Offline)))
        );
    }

    #[tokio::test]
    async fn panel_requests_reach_the_daemon() {
        let (mut daemon, stream) = daemon(None);
        let mut panel = PanelClient::start(stream).await.unwrap();

        panel
            .send(&PanelMessage::Command {
                command: CommandDto::TogglePower,
            })
            .await
            .unwrap();
        panel.send(&PanelMessage::Activate).await.unwrap();

        assert_eq!(
            daemon.requests.recv().await,
            Some(PanelMessage::Command {
                command: CommandDto::TogglePower
            })
        );
        assert_eq!(daemon.requests.recv().await, Some(PanelMessage::Activate));
    }

    #[tokio::test]
    async fn an_incompatible_panel_is_told_to_shut_down() {
        let (mut daemon, mut stream) = daemon(Some(snapshot(ConnectionDto::Online)));

        stream
            .write_all(&encode(&PanelMessage::Hello {
                protocol_version: PROTOCOL_VERSION + 1,
            }))
            .await
            .unwrap();
        let mut client = PanelClient {
            stream,
            decoder: LineDecoder::new(),
        };

        assert_eq!(
            client.receive().await.unwrap(),
            Some(DaemonMessage::Shutdown)
        );
        assert_eq!(client.receive().await.unwrap(), None, "conexão fechada");
        daemon.served.await.unwrap();
        assert!(daemon.requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_connection_that_does_not_start_with_hello_is_closed() {
        let (mut daemon, mut stream) = daemon(None);

        stream
            .write_all(&encode(&PanelMessage::Activate))
            .await
            .unwrap();

        daemon.served.await.unwrap();
        assert!(daemon.requests.try_recv().is_err(), "nada repassado");
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_connection_is_closed_after_the_hello_timeout() {
        let (daemon, _stream) = daemon(None);

        tokio::time::sleep(HELLO_TIMEOUT + Duration::from_millis(1)).await;

        daemon.served.await.unwrap();
    }

    #[tokio::test]
    async fn the_shutdown_signal_reaches_the_panel() {
        let (daemon, stream) = daemon(None);
        let mut panel = PanelClient::start(stream).await.unwrap();

        daemon.shutdown.send_replace(true);

        assert_eq!(
            panel.receive().await.unwrap(),
            Some(DaemonMessage::Shutdown)
        );
        assert_eq!(panel.receive().await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_daemon_side_ends_when_the_panel_closes() {
        let (daemon, stream) = daemon(None);
        let panel = PanelClient::start(stream).await.unwrap();

        drop(panel);

        daemon.served.await.unwrap();
    }
}
