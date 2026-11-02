//! Adaptador: persistência da configuração em `config.toml`.
//!
//! [`TomlConfigRepository`] implementa a porta `ConfigRepository`. O formato do arquivo está
//! documentado no módulo `repository`.

mod repository;
mod store;

pub use repository::TomlConfigRepository;
pub use store::{FileStore, FsStore};
