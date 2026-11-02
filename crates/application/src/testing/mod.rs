//! Dublês de teste das portas, escritos à mão.
//!
//! Os dublês compartilham o estado interno entre clones (`Rc`), então o teste entrega um clone ao
//! caso de uso e guarda outro para inspecionar ou roteirizar.

mod clock;
mod config;
mod connection;
mod gateway;
mod observer;

pub use clock::ManualClock;
pub use config::FakeConfigRepository;
pub use connection::{ConnectAttempt, ScriptedEvents, StubConnector, StubDiscovery};
pub use gateway::FakeBulbGateway;
pub use observer::SpyObserver;
