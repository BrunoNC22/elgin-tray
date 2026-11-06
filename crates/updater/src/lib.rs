//! Adaptador: releases do GitHub e instalação das atualizações com verificação de assinatura.
//!
//! As releases são assinadas com o [zipsign](https://github.com/Kijewski/zipsign) no workflow de
//! release (`.github/workflows/release.yml`); o app só instala pacotes assinados pela chave privada
//! que corresponde a [`VERIFYING_KEY`]. A fonte de versões ([`GitHubReleases`]) serve os dois
//! sistemas; o instalador ([`ReleaseInstaller`]) só é usado no Windows (no Arch quem instala é o
//! pacote do AUR).

// Tudo roda no runtime de uma thread só do daemon (ver `elgin-application`): sem `Send` nas futures.
#![allow(async_fn_in_trait)]

mod github;
mod http;
mod installer;
mod package;

pub use github::{GitHubReleases, LATEST_RELEASE_URL};
pub use http::{HttpGet, HttpResponse, UreqHttp};
pub use installer::{ExecutableReplacer, ReleaseInstaller, SelfReplace};
pub use package::{PackageSpec, WINDOWS_X86_64};

/// Chave pública (ed25519) que confere a assinatura das releases. A chave privada fica fora do
/// repositório, no segredo `ZIPSIGN_PRIVATE_KEY` do GitHub (ver `docs/releasing.md`).
pub const VERIFYING_KEY: &[u8; 32] = include_bytes!("../keys/zipsign.pub");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verifying_key_is_a_valid_ed25519_key() {
        assert!(zipsign_api::VerifyingKey::from_bytes(VERIFYING_KEY).is_ok());
    }
}
