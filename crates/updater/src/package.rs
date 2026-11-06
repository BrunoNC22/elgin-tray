//! O pacote de uma release: qual arquivo baixar, conferir a assinatura e tirar o executável.

use std::io::{Cursor, Read};

use elgin_application::ports::{Release, ReleaseAsset, UpdateError};
use elgin_application::updates::Version;
use zipsign_api::VerifyingKey;
use zipsign_api::verify::verify_zip;

/// O pacote de um sistema, como o workflow de release gera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageSpec {
    /// Sufixo do nome do pacote (`windows-x86_64`).
    pub platform: &'static str,
    /// O executável dentro do pacote.
    pub executable: &'static str,
}

/// O pacote do Windows (`.zip` com o `.exe` na raiz).
pub const WINDOWS_X86_64: PackageSpec = PackageSpec {
    platform: "windows-x86_64",
    executable: "elgin-tray.exe",
};

impl PackageSpec {
    /// Nome do arquivo na release: `elgin-tray-vX.Y.Z-<plataforma>.zip`.
    pub fn asset_name(self, version: Version) -> String {
        format!("elgin-tray-v{version}-{}.zip", self.platform)
    }

    /// O arquivo deste sistema na release.
    pub fn select(self, release: &Release) -> Result<&ReleaseAsset, UpdateError> {
        let name = self.asset_name(release.version);
        release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| UpdateError::Rejected(format!("a release não tem o arquivo {name}")))
    }
}

/// Confere a assinatura zipsign de `package`, que precisa ter sido feita com o nome `asset_name`
/// (o contexto padrão do zipsign) por uma chave que corresponde a `verifying_key`.
pub(crate) fn verify_signature(
    package: &[u8],
    asset_name: &str,
    verifying_key: &[u8; 32],
) -> Result<(), UpdateError> {
    let key = VerifyingKey::from_bytes(verifying_key)
        .map_err(|error| UpdateError::Rejected(format!("chave pública inválida ({error})")))?;
    verify_zip(
        &mut Cursor::new(package),
        &[key],
        Some(asset_name.as_bytes()),
    )
    .map(|_| ())
    .map_err(|error| UpdateError::Rejected(format!("assinatura inválida ({error})")))
}

/// Tira `executable` de dentro do `.zip`.
pub(crate) fn extract(package: &[u8], executable: &str) -> Result<Vec<u8>, UpdateError> {
    let rejected = |what: String| UpdateError::Rejected(what);
    let mut archive = zip::ZipArchive::new(Cursor::new(package))
        .map_err(|error| rejected(format!("pacote ilegível ({error})")))?;
    let mut file = archive
        .by_name(executable)
        .map_err(|_| rejected(format!("o pacote não tem {executable}")))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| rejected(format!("pacote corrompido ({error})")))?;
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SIGNED: &[u8] =
        include_bytes!("../tests/fixtures/elgin-tray-v9.9.9-windows-x86_64.zip");
    pub(crate) const TAMPERED: &[u8] =
        include_bytes!("../tests/fixtures/tampered-elgin-tray-v9.9.9-windows-x86_64.zip");
    pub(crate) const TEST_KEY: &[u8; 32] = include_bytes!("../tests/fixtures/test-key.pub");
    pub(crate) const NAME: &str = "elgin-tray-v9.9.9-windows-x86_64.zip";
    pub(crate) const EXECUTABLE: &[u8] = "executável de teste v9.9.9\n".as_bytes();

    fn release(names: &[&str]) -> Release {
        Release {
            version: "9.9.9".parse().unwrap(),
            assets: names
                .iter()
                .map(|name| ReleaseAsset {
                    name: (*name).to_owned(),
                    url: format!("https://example.com/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn picks_the_package_of_this_system() {
        let release = release(&[
            "elgin-tray-v9.9.9-linux-x86_64.tar.gz",
            NAME,
            "SHA256SUMS.txt",
        ]);

        assert_eq!(WINDOWS_X86_64.select(&release).unwrap().name, NAME);
    }

    #[test]
    fn a_release_without_the_package_is_rejected() {
        let release = release(&["elgin-tray-v9.9.9-linux-x86_64.tar.gz"]);

        assert!(matches!(
            WINDOWS_X86_64.select(&release),
            Err(UpdateError::Rejected(reason)) if reason.contains(NAME)
        ));
    }

    #[test]
    fn accepts_a_package_signed_by_the_key() {
        assert_eq!(verify_signature(SIGNED, NAME, TEST_KEY), Ok(()));
    }

    #[test]
    fn rejects_a_tampered_package() {
        assert!(matches!(
            verify_signature(TAMPERED, NAME, TEST_KEY),
            Err(UpdateError::Rejected(reason)) if reason.contains("assinatura")
        ));
    }

    #[test]
    fn rejects_a_package_under_another_name_or_key() {
        let other_name = "elgin-tray-v9.9.8-windows-x86_64.zip";
        assert!(verify_signature(SIGNED, other_name, TEST_KEY).is_err());
        assert!(verify_signature(SIGNED, NAME, crate::VERIFYING_KEY).is_err());
    }

    #[test]
    fn extracts_the_executable() {
        assert_eq!(extract(SIGNED, "elgin-tray.exe").unwrap(), EXECUTABLE);
        assert!(extract(SIGNED, "outro.exe").is_err());
        assert!(extract(b"isto nao e um zip", "elgin-tray.exe").is_err());
    }
}
