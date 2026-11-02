//! Portas: o que a aplicação precisa do mundo de fora. Os adaptadores implementam estes traits.

mod autostart;
mod clock;
mod cloud;
mod config;
mod connection;
mod gateway;
mod hotkeys;
mod notifier;
mod observer;

pub use autostart::AutostartRegistrar;
pub use clock::Clock;
pub use cloud::{CloudAccount, CloudDevice, CloudDevices, CloudError, LoginPoll, QrLogin};
pub use config::{ConfigError, ConfigRepository};
pub use connection::{BulbConnector, BulbDiscovery, BulbEvent, BulbEvents};
pub use gateway::{BulbGateway, GatewayError};
pub use hotkeys::HotkeyRegistrar;
pub use notifier::Notifier;
pub use observer::{BulbObserver, ControllerEvent};
