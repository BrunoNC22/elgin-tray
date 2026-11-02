//! Meio por onde os bytes passam. No app é o TCP; nos testes, uma lâmpada falsa.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Envia e recebe bytes.
///
/// `receive` precisa poder ser cancelado sem perder dados (é usado dentro de `select!`).
pub(crate) trait Transport {
    async fn send(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Lê o que chegou para `buffer`; `Ok(0)` quando a outra ponta fechou a conexão.
    async fn receive(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
}

impl Transport for TcpStream {
    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_all(bytes).await
    }

    async fn receive(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.read(buffer).await
    }
}
