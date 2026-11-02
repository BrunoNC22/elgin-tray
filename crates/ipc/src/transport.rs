//! Transporte local entre os processos do app, e a trava de instância única.
//!
//! - Windows: named pipe `\\.\pipe\elgin-tray-<usuário>`.
//! - Linux: socket `$XDG_RUNTIME_DIR/elgin-tray.sock` (pasta privada do usuário).
//!
//! O próprio endpoint é a trava de instância única: se alguém já atende nele, este processo não é o
//! daemon. Esta parte é fina de propósito; a conversa está em `session` (testada sem SO).

use std::io;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::{Listener, Stream};
use interprocess::local_socket::{ListenerOptions, Name};

use crate::messages::PanelMessage;
use crate::session::PanelClient;

/// Conexão com o outro processo.
pub type IpcStream = Stream;

/// Endpoint do daemon, aceitando conexões.
#[derive(Debug)]
pub struct IpcListener {
    listener: Listener,
}

impl IpcListener {
    pub async fn accept(&self) -> io::Result<IpcStream> {
        self.listener.accept().await
    }
}

/// Resultado de tentar ser o daemon.
#[derive(Debug)]
pub enum Instance {
    /// Este processo é o daemon: atenda neste endpoint.
    Primary(IpcListener),
    /// Já havia um daemon; ele foi avisado para abrir o painel.
    AlreadyRunning,
}

/// Vira o daemon, ou avisa o daemon que já existe (`Activate`) e devolve `AlreadyRunning`.
pub async fn claim() -> io::Result<Instance> {
    if let Ok(stream) = connect().await {
        let mut client = PanelClient::start(stream).await?;
        client.send(&PanelMessage::Activate).await?;
        return Ok(Instance::AlreadyRunning);
    }
    // Ninguém atende: no Linux pode sobrar o arquivo de uma execução que caiu; ele é sobrescrito.
    let listener = ListenerOptions::new()
        .name(endpoint()?)
        .try_overwrite(true)
        .create_tokio()?;
    Ok(Instance::Primary(IpcListener { listener }))
}

/// Conecta no daemon (usado pelo painel).
pub async fn connect() -> io::Result<IpcStream> {
    Stream::connect(endpoint()?).await
}

#[cfg(windows)]
fn endpoint() -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};
    let user = std::env::var("USERNAME").unwrap_or_default();
    format!("elgin-tray-{}", sanitize(&user))
        .to_ns_name::<GenericNamespaced>()
        .map(Name::into_owned)
}

#[cfg(not(windows))]
fn endpoint() -> io::Result<Name<'static>> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    // Sem XDG_RUNTIME_DIR (raro no KDE), usa a pasta temporária com o nome do usuário.
    let path = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => std::path::PathBuf::from(dir).join("elgin-tray.sock"),
        None => {
            let user = std::env::var("USER").unwrap_or_default();
            std::env::temp_dir().join(format!("elgin-tray-{}.sock", sanitize(&user)))
        }
    };
    path.to_fs_name::<GenericFilePath>().map(Name::into_owned)
}

/// Só letras, números, `-` e `_` no nome do endpoint.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_only_safe_characters() {
        assert_eq!(sanitize("João Silva/../x"), "Jo_o_Silva____x");
        assert_eq!(sanitize("the_b-1"), "the_b-1");
    }
}
