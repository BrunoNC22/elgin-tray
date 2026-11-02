use std::time::Duration;

use elgin_domain::{BulbId, BulbState};

use crate::credentials::{Endpoint, LocalKey};
use crate::ports::{BulbGateway, GatewayError};

/// Abre conexões com a lâmpada.
///
/// Uma conexão tem dois lados: o gateway (pedidos: ler estado, aplicar estado), que fica com o
/// controlador, e os eventos (o que a lâmpada avisa sozinha), que o daemon espera.
pub trait BulbConnector {
    type Gateway: BulbGateway;
    type Events: BulbEvents;

    /// Conecta e autentica. Erros: `Unreachable`, `Timeout`, `KeyRejected`, `Protocol`.
    async fn connect(
        &mut self,
        id: &BulbId,
        key: &LocalKey,
        endpoint: &Endpoint,
    ) -> Result<(Self::Gateway, Self::Events), GatewayError>;
}

/// O que a lâmpada avisa sem ter sido perguntada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BulbEvent {
    /// O estado mudou (outro app, interruptor, ou a confirmação de um comando nosso).
    StateReported(BulbState),
    /// A conexão caiu. Depois deste evento não chegam outros.
    ConnectionLost(GatewayError),
}

/// Fluxo de eventos de uma conexão.
pub trait BulbEvents {
    /// Espera o próximo evento.
    async fn next_event(&mut self) -> BulbEvent;
}

/// Procura a lâmpada na rede local pelo identificador.
pub trait BulbDiscovery {
    /// Espera até `timeout` pelo anúncio da lâmpada; `None` se ela não apareceu.
    async fn find(&mut self, id: &BulbId, timeout: Duration) -> Option<Endpoint>;
}
