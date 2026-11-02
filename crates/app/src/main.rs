//! `elgin-tray`: daemon da bandeja (padrão), painel (`panel`) e CLI de desenvolvimento (`ctl`).
//!
//! Este é o composition root: o único lugar que conhece todos os adaptadores e escolhe os de cada
//! sistema por `cfg(target_os)`. A montagem do daemon chega na #21.

mod args;
mod console;
mod ctl;

use std::path::PathBuf;
use std::process::ExitCode;

use elgin_application::config::Settings;
use elgin_config::{FsStore, TomlConfigRepository};
use elgin_tuya_cloud::{SystemEnv, TuyaCloud, UreqClient};
use elgin_tuya_lan::{TuyaDiscovery, TuyaLanConnector};

use crate::args::{CtlCommand, Mode, USAGE};

/// Variável de ambiente para usar outro `config.toml` (desenvolvimento e testes manuais).
const CONFIG_OVERRIDE: &str = "ELGIN_TRAY_CONFIG";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&args) {
        Ok(Mode::Ctl(command)) => {
            console::attach_parent_console();
            run_ctl(command)
        }
        Ok(Mode::Help) => {
            console::attach_parent_console();
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Mode::Daemon | Mode::Panel) => {
            console::attach_parent_console();
            eprintln!("O app da bandeja ainda não foi implementado. Use `elgin-tray ctl`.");
            ExitCode::from(2)
        }
        Err(message) => {
            console::attach_parent_console();
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_ctl(command: CtlCommand) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Erro: não foi possível iniciar o runtime ({error})");
            return ExitCode::FAILURE;
        }
    };
    // As conexões Tuya rodam em tarefas locais: tudo dentro de um LocalSet.
    let result = tokio::task::LocalSet::new().block_on(&runtime, async {
        let repository = match std::env::var_os(CONFIG_OVERRIDE) {
            Some(path) => TomlConfigRepository::new(FsStore, PathBuf::from(path)),
            None => TomlConfigRepository::at_default_location()
                .ok_or("não foi possível descobrir a pasta de configuração do usuário")?,
        };
        let path = repository.path().display().to_string();
        let mut settings =
            Settings::load(repository).map_err(|error| format!("{error} ({path})"))?;
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
