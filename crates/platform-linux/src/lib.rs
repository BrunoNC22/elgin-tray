//! Adaptadores do Linux: autostart, atalhos globais (KDE) e notificações.
//!
//! Só compila no Linux; nos outros sistemas a crate fica vazia.

#![cfg(target_os = "linux")]

mod autostart;
mod notify;

pub use autostart::{AutostartFiles, DesktopAutostart, FsAutostartFiles};
pub use notify::DesktopNotifier;
