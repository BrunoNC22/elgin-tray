//! Portas: o que a aplicação precisa do mundo de fora. Os adaptadores implementam estes traits.

mod clock;
mod connection;
mod gateway;
mod observer;

pub use clock::Clock;
pub use connection::{BulbConnector, BulbDiscovery};
pub use gateway::{BulbGateway, GatewayError};
pub use observer::{BulbObserver, ControllerEvent};
