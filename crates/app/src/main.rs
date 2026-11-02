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

use elgin_application::config::Settings;
use elgin_config::{FsStore, TomlConfigRepository};
use elgin_ipc::session::DaemonChannels;
use elgin_ipc::transport::{self, Instance};
use elgin_tuya_cloud::{SystemEnv, TuyaCloud, UreqClient};
use elgin_tuya_lan::{TuyaDiscovery, TuyaLanConnector};
use tokio::sync::{mpsc, watch};

use crate::args::{CtlCommand, Mode, USAGE};
use crate::daemon::{ChildPanel, DaemonCore};

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
        Ok(Mode::Panel) => {
            console::attach_parent_console();
            eprintln!("O painel ainda não foi implementado.");
            ExitCode::from(2)
        }
        Err(message) => {
            console::attach_parent_console();
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_daemon() -> ExitCode {
    let log_path = logging::init();
    log::info!("elgin-tray {} iniciando", env!("CARGO_PKG_VERSION"));
    let result = block_on_local(async {
        let listener = match transport::claim()
            .await
            .map_err(|error| format!("não foi possível abrir o canal local: {error}"))?
        {
            Instance::Primary(listener) => listener,
            Instance::AlreadyRunning => {
                log::info!("já havia um elgin-tray rodando; pedido para abrir o painel enviado");
                return Ok(());
            }
        };
        let settings = load_settings()?;
        let (requests_sender, requests) = mpsc::unbounded_channel();
        let (snapshots_sender, snapshots) = watch::channel(None);
        let (shutdown_sender, shutdown) = watch::channel(false);
        let core = DaemonCore::new(
            settings,
            TuyaLanConnector::default(),
            TuyaDiscovery::default(),
            TuyaCloud::new(UreqClient::default(), SystemEnv),
            ChildPanel::default(),
            snapshots_sender,
        );
        tokio::task::spawn_local(daemon::accept_panels(
            listener,
            DaemonChannels {
                requests: requests_sender,
                snapshots,
                shutdown,
            },
        ));
        // Por enquanto só Ctrl+C encerra; o item "Sair" da bandeja chega na #23/#24.
        daemon::run(core, requests, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
        let _ = shutdown_sender.send(true);
        log::info!("elgin-tray encerrado");
        Ok(())
    });
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
