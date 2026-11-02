//! `elgin-tray`: daemon da bandeja (padrão), painel (`panel`) e CLI de desenvolvimento (`ctl`).
//!
//! Este é o composition root: o único lugar que conhece todos os adaptadores e escolhe os de cada
//! sistema por `cfg(target_os)`.

// No Windows, o executável de release não abre console (é um app de bandeja). Os builds de
// desenvolvimento continuam com console para facilitar a depuração.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod console;
mod ctl;
mod daemon;
mod logging;

use std::future::Future;
use std::path::PathBuf;
use std::process::ExitCode;

use elgin_application::config::{HotkeyBindings, Settings, apply_autostart};
use elgin_application::hotkeys::HotkeyAction;
#[cfg(windows)]
use elgin_application::hotkeys::register_hotkeys;
use elgin_application::ports::AutostartRegistrar;
use elgin_application::tray::UpdatePolicy;
use elgin_config::{FsStore, TomlConfigRepository};
use elgin_ipc::session::DaemonChannels;
use elgin_ipc::transport::{self, Instance};
use elgin_tuya_cloud::{SystemEnv, TuyaCloud, UreqClient};
use elgin_tuya_lan::{TuyaDiscovery, TuyaLanConnector};
use tokio::sync::{mpsc, watch};

use crate::args::{CtlCommand, Mode, USAGE};
use crate::daemon::{Adapters, ChildPanel, DaemonCore};

/// Como as atualizações chegam: no Windows o app instala sozinho; no Linux (pacote AUR) só avisa.
const UPDATE_POLICY: UpdatePolicy = if cfg!(windows) {
    UpdatePolicy::AutoInstall
} else {
    UpdatePolicy::NotifyOnly
};

/// Variável de ambiente para usar outro `config.toml` (desenvolvimento e testes manuais).
const CONFIG_OVERRIDE: &str = "ELGIN_TRAY_CONFIG";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&args) {
        Ok(Mode::Daemon) => run_daemon(),
        Ok(Mode::Ctl(command)) => {
            console::attach_parent_console();
            run_ctl(command)
        }
        Ok(Mode::Help) => {
            console::attach_parent_console();
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Mode::Panel(anchor)) => match elgin_panel::runner::run(anchor) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                console::attach_parent_console();
                eprintln!("Erro: {message}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            console::attach_parent_console();
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// No Windows a bandeja precisa do loop de mensagens na thread principal: o daemon roda numa thread
/// própria e conversa com a bandeja pelo [`elgin_tray_windows::SharedView`].
#[cfg(windows)]
fn run_daemon() -> ExitCode {
    use std::sync::{Arc, Mutex};

    let log_path = logging::init();
    log::info!("elgin-tray {} iniciando", env!("CARGO_PKG_VERSION"));
    let shared: elgin_tray_windows::SharedView = Arc::new(Mutex::new(None));
    let (tray_sender, tray_events) = mpsc::unbounded_channel();
    let (waker_sender, waker_receiver) = std::sync::mpsc::channel();
    let (started_sender, started) = std::sync::mpsc::channel();

    let daemon_shared = Arc::clone(&shared);
    let daemon = std::thread::spawn(move || {
        block_on_local(async move {
            let Some(listener) = claim_instance().await? else {
                let _ = started_sender.send(false);
                return Ok(());
            };
            let _ = started_sender.send(true);
            let waker: elgin_tray_windows::TrayWaker = waker_receiver
                .recv()
                .map_err(|_| "a bandeja não iniciou".to_owned())?;
            let mut tray = WindowsTray {
                shared: daemon_shared,
                waker,
            };
            let result = serve(listener, tray_events, &mut tray).await;
            waker.quit();
            result
        })
    });

    let tray_result = match started.recv() {
        Ok(true) => elgin_tray_windows::run(
            shared,
            move |action, rect| {
                let anchor = rect.map(|rect| daemon::Anchor {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                });
                let _ = tray_sender.send(daemon::TrayEvent { action, anchor });
            },
            move |waker| {
                let _ = waker_sender.send(waker);
            },
        ),
        _ => Ok(()),
    };
    let daemon_result = daemon
        .join()
        .unwrap_or_else(|_| Err("a thread do daemon falhou".to_owned()));
    finish(tray_result.and(daemon_result), log_path)
}

/// A bandeja do Windows vista pelo daemon: guarda o estado e acorda o loop de mensagens.
#[cfg(windows)]
struct WindowsTray {
    shared: elgin_tray_windows::SharedView,
    waker: elgin_tray_windows::TrayWaker,
}

#[cfg(windows)]
impl daemon::TrayOutput for WindowsTray {
    fn show(&mut self, view: &elgin_application::tray::TrayViewModel) {
        if let Ok(mut shared) = self.shared.lock() {
            *shared = Some(view.clone());
        }
        self.waker.refresh();
    }
}

/// No Linux a bandeja do KDE chega na #25; até lá o daemon roda sem ícone.
#[cfg(not(windows))]
fn run_daemon() -> ExitCode {
    let log_path = logging::init();
    log::info!("elgin-tray {} iniciando", env!("CARGO_PKG_VERSION"));
    let result = block_on_local(async {
        let Some(listener) = claim_instance().await? else {
            return Ok(());
        };
        // Sem bandeja ainda: o remetente fica vivo para o canal não fechar.
        let (_tray_sender, tray_events) = mpsc::unbounded_channel();
        serve(listener, tray_events, &mut NoTray).await
    });
    finish(result, log_path)
}

#[cfg(not(windows))]
struct NoTray;

#[cfg(not(windows))]
impl daemon::TrayOutput for NoTray {
    fn show(&mut self, _view: &elgin_application::tray::TrayViewModel) {}
}

/// Vira o daemon (`Some(listener)`) ou avisa o que já está rodando (`None`).
async fn claim_instance() -> Result<Option<transport::IpcListener>, String> {
    match transport::claim()
        .await
        .map_err(|error| format!("não foi possível abrir o canal local: {error}"))?
    {
        Instance::Primary(listener) => Ok(Some(listener)),
        Instance::AlreadyRunning => {
            log::info!("já havia um elgin-tray rodando; pedido para abrir o painel enviado");
            Ok(None)
        }
    }
}

/// Monta o daemon com os adaptadores reais e roda até "Sair" (ou Ctrl+C, em desenvolvimento).
async fn serve(
    listener: transport::IpcListener,
    tray_events: mpsc::UnboundedReceiver<daemon::TrayEvent>,
    tray: &mut impl daemon::TrayOutput,
) -> Result<(), String> {
    let mut settings = load_settings()?;
    let mut autostart = autostart_registrar()?;
    // Em desenvolvimento, não registra o executável de `target/` para iniciar com o sistema.
    if cfg!(not(debug_assertions)) {
        match apply_autostart(&mut settings, &mut autostart, true) {
            Ok(enabled) => log::info!("iniciar com o sistema: {enabled}"),
            Err(error) => log::warn!("não foi possível aplicar o autostart: {error}"),
        }
    }
    let (hotkey_sender, hotkey_events) = mpsc::unbounded_channel();
    let _hotkeys = start_hotkeys(&settings.config().hotkeys, hotkey_sender);
    let (requests_sender, requests) = mpsc::unbounded_channel();
    let (snapshots_sender, snapshots) = watch::channel(None);
    let (shutdown_sender, shutdown) = watch::channel(false);
    let core = DaemonCore::new(
        settings,
        Adapters {
            connector: TuyaLanConnector::default(),
            discovery: TuyaDiscovery::default(),
            cloud: TuyaCloud::new(UreqClient::default(), SystemEnv),
            panel: ChildPanel::default(),
            autostart,
        },
        snapshots_sender,
        UPDATE_POLICY,
    );
    tokio::task::spawn_local(daemon::accept_panels(
        listener,
        DaemonChannels {
            requests: requests_sender,
            snapshots,
            shutdown,
        },
    ));
    daemon::run(core, requests, tray_events, hotkey_events, tray, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await;
    let _ = shutdown_sender.send(true);
    log::info!("elgin-tray encerrado");
    Ok(())
}

#[cfg(windows)]
fn autostart_registrar() -> Result<impl AutostartRegistrar, String> {
    elgin_platform_windows::WindowsAutostart::for_current_exe()
        .map_err(|error| format!("caminho do executável: {error}"))
}

#[cfg(target_os = "linux")]
fn autostart_registrar() -> Result<impl AutostartRegistrar, String> {
    elgin_platform_linux::DesktopAutostart::for_current_exe()
        .map_err(|error| format!("pasta de autostart: {error}"))
}

/// Registra os atalhos globais do config; os apertados chegam por `sender`. O valor devolvido
/// precisa ficar vivo enquanto o daemon roda. Atalho inválido ou já usado vai para o log.
#[cfg(windows)]
fn start_hotkeys(
    bindings: &HotkeyBindings,
    sender: mpsc::UnboundedSender<HotkeyAction>,
) -> Option<elgin_platform_windows::WindowsHotkeys> {
    let started = elgin_platform_windows::WindowsHotkeys::start(move |action| {
        let _ = sender.send(action);
    });
    let mut hotkeys = started
        .inspect_err(|error| log::warn!("atalhos globais desligados: {error}"))
        .ok()?;
    for problem in register_hotkeys(bindings, &mut hotkeys) {
        log::warn!("{problem}");
    }
    Some(hotkeys)
}

/// Os atalhos do KDE chegam na #35; até lá não há nada para manter vivo.
#[cfg(not(windows))]
fn start_hotkeys(
    _bindings: &HotkeyBindings,
    _sender: mpsc::UnboundedSender<HotkeyAction>,
) -> Option<()> {
    None
}

fn finish(result: Result<(), String>, log_path: Option<PathBuf>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            log::error!("{message}");
            if let Some(path) = log_path {
                eprintln!("Erro: {message} (log em {})", path.display());
            }
            ExitCode::FAILURE
        }
    }
}

fn run_ctl(command: CtlCommand) -> ExitCode {
    let result = block_on_local(async {
        let mut settings = load_settings()?;
        ctl::run(
            command,
            &mut settings,
            &mut TuyaLanConnector::default(),
            &mut TuyaDiscovery::default(),
            &mut TuyaCloud::new(UreqClient::default(), SystemEnv),
            &mut ctl::Stdout,
        )
        .await
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Erro: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Roda `future` num runtime de uma thread dentro de um `LocalSet` (as conexões Tuya e os painéis
/// rodam em tarefas locais).
fn block_on_local(future: impl Future<Output = Result<(), String>>) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("não foi possível iniciar o runtime ({error})"))?;
    tokio::task::LocalSet::new().block_on(&runtime, future)
}

fn load_settings() -> Result<Settings<TomlConfigRepository>, String> {
    let repository = match std::env::var_os(CONFIG_OVERRIDE) {
        Some(path) => TomlConfigRepository::new(FsStore, PathBuf::from(path)),
        None => TomlConfigRepository::at_default_location()
            .ok_or("não foi possível descobrir a pasta de configuração do usuário")?,
    };
    let path = repository.path().display().to_string();
    Settings::load(repository).map_err(|error| format!("{error} ({path})"))
}
