//! Portas: o que a aplicação precisa do mundo de fora. Os adaptadores implementam estes traits.

mod clock;
mod gateway;
mod observer;

pub use clock::Clock;
pub use gateway::{BulbGateway, GatewayError};
pub use observer::{BulbObserver, ControllerEvent};
