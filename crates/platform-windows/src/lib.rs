//! Adaptadores do Windows: autostart, atalhos globais, notificações e instalação de atualizações.
//!
//! Só compila no Windows; nos outros sistemas a crate fica vazia.

#![cfg(windows)]

mod autostart;

pub use autostart::{HkcuRunKey, RunKey, WindowsAutostart};
