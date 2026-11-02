//! O daemon da bandeja: dono da conexão com a lâmpada, da configuração e dos painéis.

mod clock;
mod core;
mod panel;

use std::future::Future;
use std::time::Instant;

use elgin_application::ports::{BulbConnector, BulbDiscovery, CloudAccount, ConfigRepository};
use elgin_ipc::messages::PanelMessage;
use elgin_ipc::session::{DaemonChannels, serve_client};
use elgin_ipc::transport::IpcListener;
use tokio::sync::mpsc;

pub(crate) use self::core::DaemonCore;
pub(crate) use self::panel::ChildPanel;

/// O loop do daemon: entrega ao núcleo os prazos, os eventos da lâmpada e os pedidos dos painéis,
/// até `exit` terminar.
pub(crate) async fn run<R, K, D, C, P>(
    mut core: DaemonCore<R, K, D, C, P>,
    mut requests: mpsc::UnboundedReceiver<PanelMessage>,
    exit: impl Future<Output = ()>,
) where
    R: ConfigRepository,
    K: BulbConnector + Clone,
    D: BulbDiscovery + Clone,
    C: CloudAccount + Clone,
    P: panel::PanelProcess,
{
    tokio::pin!(exit);
    loop {
        let deadline = core.next_deadline();
        tokio::select! {
            () = wait_until(deadline) => core.on_deadline().await,
            event = core.next_bulb_event() => core.on_bulb_event(event).await,
            Some(message) = requests.recv() => core.on_request(message).await,
            () = &mut exit => break,
        }
    }
    core.shutdown();
}

/// Aceita painéis numa tarefa própria (aceitar dentro de um `select!` poderia perder conexões).
pub(crate) async fn accept_panels(listener: IpcListener, channels: DaemonChannels) {
    loop {
        match listener.accept().await {
            Ok(stream) => {
                tokio::task::spawn_local(serve_client(stream, channels.clone()));
            }
            Err(error) => {
                log::warn!("falha ao aceitar conexão local: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
    }
}
