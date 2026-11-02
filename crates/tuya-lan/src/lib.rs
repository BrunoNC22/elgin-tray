//! Adaptador: protocolo local Tuya 3.5 (TCP 6668) e descoberta UDP.
//!
//! Referência do protocolo: tinytuya (ver `tools/tuya-vectors/README.md`).
//!
//! [`TuyaLanConnector`] implementa a porta `BulbConnector`. Cada conexão roda um ator numa tarefa
//! local do tokio, então o conector precisa ser usado dentro de um `tokio::task::LocalSet`.

mod connection;
mod connector;
mod discovery;
mod dps;
mod frame;
mod message;
mod random;
mod session;
mod transport;

pub use connection::{Timing, TuyaEvents, TuyaGateway};
pub use connector::TuyaLanConnector;
pub use discovery::TuyaDiscovery;

#[cfg(test)]
mod fake_lamp;
#[cfg(test)]
mod test_vectors;
