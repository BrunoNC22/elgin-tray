use std::time::Duration;

use elgin_domain::BulbId;

use crate::credentials::{Endpoint, LocalKey};
use crate::ports::{BulbGateway, GatewayError};

/// Abre conexões com a lâmpada.
pub trait BulbConnector {
    type Gateway: BulbGateway;

    /// Conecta e autentica. Erros: `Unreachable`, `Timeout`, `KeyRejected`, `Protocol`.
    async fn connect(
        &mut self,
        id: &BulbId,
        key: &LocalKey,
        endpoint: &Endpoint,
    ) -> Result<Self::Gateway, GatewayError>;
}

/// Procura a lâmpada na rede local pelo identificador.
pub trait BulbDiscovery {
    /// Espera até `timeout` pelo anúncio da lâmpada; `None` se ela não apareceu.
    async fn find(&mut self, id: &BulbId, timeout: Duration) -> Option<Endpoint>;
}
