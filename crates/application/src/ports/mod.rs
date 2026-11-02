//! Portas: o que a aplicação precisa do mundo de fora. Os adaptadores implementam estes traits.

mod clock;
mod cloud;
mod config;
mod connection;
mod gateway;
mod observer;

pub use clock::Clock;
pub use cloud::{CloudAccount, CloudError, LoginPoll, QrLogin};
pub use config::{ConfigError, ConfigRepository};
pub use connection::{BulbConnector, BulbDiscovery, BulbEvent, BulbEvents};
pub use gateway::{BulbGateway, GatewayError};
pub use observer::{BulbObserver, ControllerEvent};
