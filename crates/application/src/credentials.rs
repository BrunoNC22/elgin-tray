//! Dados de acesso à lâmpada. São segredos: nunca aparecem em `Debug` nem em logs.

use std::fmt;
use std::net::IpAddr;

/// Chave local da lâmpada (na Tuya, a `local_key`). Segredo.
#[derive(Clone, PartialEq, Eq)]
pub struct LocalKey(String);

impl LocalKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// O valor de verdade. Use só para falar com a lâmpada ou salvar a configuração.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalKey(***)")
    }
}

/// Onde a lâmpada está na rede e qual versão do protocolo ela fala (ex.: "3.5").
///
/// A versão é opaca para a aplicação: só o adaptador sabe interpretar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub ip: IpAddr,
    pub protocol_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_key_is_hidden_in_debug() {
        let key = LocalKey::new("abcdefghijklmnop");

        assert_eq!(format!("{key:?}"), "LocalKey(***)");
        assert_eq!(key.expose(), "abcdefghijklmnop");
    }
}
