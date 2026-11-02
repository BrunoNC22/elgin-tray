//! Dublês de teste das portas, escritos à mão.
//!
//! Os dublês compartilham o estado interno entre clones (`Rc`), então o teste entrega um clone ao
//! caso de uso e guarda outro para inspecionar ou roteirizar.

mod autostart;
mod clock;
mod cloud;
mod config;
mod connection;
mod gateway;
mod observer;

pub use autostart::SpyAutostart;
pub use clock::ManualClock;
pub use cloud::StubCloudAccount;
pub use config::FakeConfigRepository;
pub use connection::{ConnectAttempt, ScriptedEvents, StubConnector, StubDiscovery};
pub use gateway::FakeBulbGateway;
pub use observer::SpyObserver;
