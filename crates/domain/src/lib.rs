//! Domínio: entidades, value objects e regras puras da lâmpada (sem I/O, sem async, sem plataforma).
//!
//! O domínio não conhece Tuya nem DPs: fala de lâmpada ligada, cor, brilho e temperatura.
//! Valores com regras (faixas, normalização) só existem válidos: os campos são privados e os
//! construtores validam (`try_*`) ou limitam (`clamped_*`).

mod brightness;
mod bulb;
mod colour;
mod command;
mod error;
mod favorites;
mod percent;
mod temperature;

pub use brightness::Brightness;
pub use bulb::{Bulb, BulbId, BulbName, BulbState, LightMode, Power};
pub use colour::{Hsv, Hue, Rgb, Saturation};
pub use command::BulbCommand;
pub use error::DomainError;
pub use favorites::{FavoriteColour, FavoriteSlot, Favorites};
pub use temperature::ColorTemperature;
