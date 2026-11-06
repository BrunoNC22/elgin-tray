//! Adaptador: releases do GitHub e verificação de assinatura das atualizações.
//!
//! As releases são assinadas com o [zipsign](https://github.com/Kijewski/zipsign) no workflow de
//! release (`.github/workflows/release.yml`); o app só aceita arquivos assinados pela chave privada
//! que corresponde a [`VERIFYING_KEY`].

/// Chave pública (ed25519) que confere a assinatura das releases. A chave privada fica fora do
/// repositório, no segredo `ZIPSIGN_PRIVATE_KEY` do GitHub (ver `docs/releasing.md`).
pub const VERIFYING_KEY: &[u8; 32] = include_bytes!("../keys/zipsign.pub");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verifying_key_is_not_empty() {
        assert!(VERIFYING_KEY.iter().any(|&byte| byte != 0));
    }
}
