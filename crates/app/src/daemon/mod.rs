//! O daemon da bandeja: dono da conexão com a lâmpada, da configuração e dos painéis.

mod clock;
mod core;
mod panel;

use std::future::Future;
use std::time::Instant;

use elgin_application::hotkeys::HotkeyAction;
use elgin_application::ports::{
    AutostartRegistrar, BulbConnector, BulbDiscovery, CloudAccount, ConfigRepository, Notifier,
    UpdateInstaller, UpdateSource,
};
use elgin_application::tray::{TrayAction, TrayViewModel};
use elgin_ipc::messages::PanelMessage;
use elgin_ipc::session::{DaemonChannels, serve_client};
use elgin_ipc::transport::IpcListener;
use tokio::sync::mpsc;

pub(crate) use self::core::{Adapters, DaemonCore, TrayOutcome};
pub(crate) use self::panel::{Anchor, ChildPanel};

/// Uma interação com o ícone da bandeja.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrayEvent {
    pub(crate) action: TrayAction,
    /// Onde o ícone está na tela (para o painel abrir colado nele).
    pub(crate) anchor: Option<Anchor>,
}

/// Para onde vai o que a bandeja deve mostrar (a bandeja do sistema).
pub(crate) trait TrayOutput {
    fn show(&mut self, view: &TrayViewModel);
}

/// Por que o loop do daemon terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonExit {
    /// "Sair" (ou Ctrl+C).
    Quit,
    /// Uma atualização foi instalada: abrir o executável novo depois de liberar o canal local.
    Restart,
}

/// O loop do daemon: entrega ao núcleo os prazos, os eventos da lâmpada, os pedidos dos painéis,
/// as ações da bandeja e os atalhos globais, e mostra na bandeja cada mudança, até "Sair" ou `exit`
/// terminar.
pub(crate) async fn run<R, K, D, C, P, A, N, S, I, T>(
    mut core: DaemonCore<R, K, D, C, P, A, N, S, I>,
    mut requests: mpsc::UnboundedReceiver<PanelMessage>,
    mut tray_events: mpsc::UnboundedReceiver<TrayEvent>,
    mut hotkeys: mpsc::UnboundedReceiver<HotkeyAction>,
    tray: &mut T,
    exit: impl Future<Output = ()>,
) -> DaemonExit
where
    T: TrayOutput,
    R: ConfigRepository,
    K: BulbConnector + Clone,
    D: BulbDiscovery + Clone,
    C: CloudAccount + Clone,
    P: panel::PanelProcess,
    A: AutostartRegistrar,
    N: Notifier,
    S: UpdateSource,
    I: UpdateInstaller,
{
    tokio::pin!(exit);
    core.on_start().await;
    let mut shown: Option<TrayViewModel> = None;
    let reason = loop {
        if core.restart_requested() {
            break DaemonExit::Restart;
        }
        let view = core.tray_view();
        if shown.as_ref() != Some(&view) {
            tray.show(&view);
            shown = Some(view);
        }
        let deadline = core.next_deadline();
        tokio::select! {
            () = wait_until(deadline) => core.on_deadline().await,
            event = core.next_bulb_event() => core.on_bulb_event(event).await,
            Some(message) = requests.recv() => core.on_request(message).await,
            Some(event) = tray_events.recv() => {
                if core.on_tray_action(event.action, event.anchor).await == TrayOutcome::Quit {
                    break DaemonExit::Quit;
                }
            }
            Some(action) = hotkeys.recv() => core.on_hotkey(action).await,
            () = &mut exit => break DaemonExit::Quit,
        }
    };
    core.shutdown();
    reason
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
