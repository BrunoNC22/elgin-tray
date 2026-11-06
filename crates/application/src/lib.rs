//! Aplicação: casos de uso e portas (traits). Dublês das portas na feature `testing`.
//!
//! Concorrência: o daemon roda num runtime `tokio` de uma thread só (`current_thread`), então as
//! portas usam `async fn` em traits sem exigir `Send` nas futures, e os casos de uso recebem os
//! adaptadores por generics (despacho estático).

// `async fn` em trait pública avisa que não dá para exigir `Send` das futures. Não precisamos:
// tudo roda numa thread só (ver acima).
#![allow(async_fn_in_trait)]

pub mod config;
pub mod controller;
pub mod credentials;
pub mod hotkeys;
pub mod ports;
pub mod setup;
pub mod status;
pub mod supervisor;
pub mod tray;
pub mod updates;

/// Dublês de teste das portas (stubs, spies, fakes), escritos à mão.
#[cfg(any(test, feature = "testing"))]
pub mod testing;
