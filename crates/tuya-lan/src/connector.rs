//! Conector real: abre o TCP na porta 6668 e negocia a sessão.

use std::net::SocketAddr;

use elgin_application::credentials::{Endpoint, LocalKey};
use elgin_application::ports::{BulbConnector, GatewayError};
use elgin_domain::BulbId;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::connection::{self, Timing, TuyaEvents, TuyaGateway};
use crate::dps::DpMap;
use crate::frame::Key;
use crate::random::OsRandomness;

/// Porta TCP do protocolo local Tuya.
const PORT: u16 = 6668;
/// Única versão do protocolo implementada.
const SUPPORTED_VERSION: &str = "3.5";

/// Abre conexões com lâmpadas Tuya 3.5 pela rede local. Precisa rodar dentro de um `LocalSet`.
#[derive(Debug, Clone, Default)]
pub struct TuyaLanConnector {
    timing: Timing,
}

impl TuyaLanConnector {
    pub fn new(timing: Timing) -> Self {
        Self { timing }
    }
}

impl BulbConnector for TuyaLanConnector {
    type Gateway = TuyaGateway;
    type Events = TuyaEvents;

    async fn connect(
        &mut self,
        id: &BulbId,
        key: &LocalKey,
        endpoint: &Endpoint,
    ) -> Result<(TuyaGateway, TuyaEvents), GatewayError> {
        check_version(endpoint)?;
        let key = key_bytes(key)?;
        let address = SocketAddr::new(endpoint.ip, PORT);
        // Falhar ou demorar para abrir o TCP é a mesma coisa para o usuário: a lâmpada não respondeu.
        let stream = timeout(self.timing.handshake_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| GatewayError::Unreachable)?
            .map_err(|_| GatewayError::Unreachable)?;
        // Comandos pequenos e frequentes (arrastar o slider): sem esperar para juntar pacotes.
        let _ = stream.set_nodelay(true);
        connection::open(
            stream,
            id.as_str(),
            key,
            OsRandomness,
            DpMap::default(),
            self.timing,
        )
        .await
    }
}

fn check_version(endpoint: &Endpoint) -> Result<(), GatewayError> {
    if endpoint.protocol_version == SUPPORTED_VERSION {
        Ok(())
    } else {
        Err(GatewayError::Protocol(format!(
            "versão {} do protocolo não é suportada (só {SUPPORTED_VERSION})",
            endpoint.protocol_version
        )))
    }
}

/// A local key da Tuya tem 16 caracteres e é usada como a chave AES. Outro tamanho não serve: é
/// tratado como chave recusada (leva ao fluxo de recuperação/reconfiguração).
fn key_bytes(key: &LocalKey) -> Result<Key, GatewayError> {
    key.expose()
        .as_bytes()
        .try_into()
        .map_err(|_| GatewayError::KeyRejected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn endpoint(version: &str) -> Endpoint {
        Endpoint {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            protocol_version: version.to_owned(),
        }
    }

    #[test]
    fn only_version_3_5_is_supported() {
        assert_eq!(check_version(&endpoint("3.5")), Ok(()));
        assert!(matches!(
            check_version(&endpoint("3.3")),
            Err(GatewayError::Protocol(_))
        ));
    }

    #[test]
    fn the_local_key_must_have_16_bytes() {
        assert_eq!(
            key_bytes(&LocalKey::new("0123456789abcdef")),
            Ok(*b"0123456789abcdef")
        );
        assert_eq!(
            key_bytes(&LocalKey::new("curta")),
            Err(GatewayError::KeyRejected)
        );
        assert_eq!(
            key_bytes(&LocalKey::new("0123456789abcdefX")),
            Err(GatewayError::KeyRejected)
        );
    }
}
