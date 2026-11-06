//! Instala uma release: baixa o pacote do sistema, confere a assinatura, tira o executável e troca o
//! executável em uso. Quem reinicia na versão nova é o daemon.

use std::io;
use std::path::{Path, PathBuf};

use elgin_application::ports::{Release, UpdateError, UpdateInstaller};

use crate::http::HttpGet;
use crate::package::{PackageSpec, extract, verify_signature};

/// Troca o executável em uso pelo arquivo `new_executable` (atrás de uma trait para os testes não
/// mexerem no executável de verdade).
pub trait ExecutableReplacer {
    fn replace(&mut self, new_executable: &Path) -> io::Result<()>;
}

/// Troca de verdade, com o `self-replace` (no Windows o executável em uso é renomeado e apagado
/// depois; o novo assume o caminho).
#[derive(Debug, Clone, Copy, Default)]
pub struct SelfReplace;

impl ExecutableReplacer for SelfReplace {
    fn replace(&mut self, new_executable: &Path) -> io::Result<()> {
        self_replace::self_replace(new_executable)
    }
}

/// O instalador de atualizações.
#[derive(Debug)]
pub struct ReleaseInstaller<H, X> {
    http: H,
    replacer: X,
    package: PackageSpec,
    verifying_key: [u8; 32],
    /// Onde o executável novo fica até a troca.
    staging_dir: PathBuf,
}

impl<H: HttpGet, X: ExecutableReplacer> ReleaseInstaller<H, X> {
    pub fn new(
        http: H,
        replacer: X,
        package: PackageSpec,
        verifying_key: [u8; 32],
        staging_dir: PathBuf,
    ) -> Self {
        Self {
            http,
            replacer,
            package,
            verifying_key,
            staging_dir,
        }
    }
}

impl<H: HttpGet, X: ExecutableReplacer> UpdateInstaller for ReleaseInstaller<H, X> {
    async fn install(&mut self, release: &Release) -> Result<(), UpdateError> {
        let asset = self.package.select(release)?;
        log::info!("baixando {}", asset.name);
        let response = self
            .http
            .get(&asset.url)
            .await
            .map_err(UpdateError::Network)?;
        if response.status != 200 {
            return Err(UpdateError::Network(format!(
                "download respondeu {}",
                response.status
            )));
        }
        verify_signature(&response.body, &asset.name, &self.verifying_key)?;
        let executable = extract(&response.body, self.package.executable)?;

        let staged = self
            .staging_dir
            .join(format!("elgin-tray-{}.new", release.version));
        let replaced = std::fs::create_dir_all(&self.staging_dir)
            .and_then(|()| std::fs::write(&staged, &executable))
            .and_then(|()| self.replacer.replace(&staged));
        let _ = std::fs::remove_file(&staged);
        replaced.map_err(|error| {
            UpdateError::Rejected(format!("não foi possível trocar o executável ({error})"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::FakeHttp;
    use crate::package::WINDOWS_X86_64;
    use crate::package::tests::{EXECUTABLE, NAME, SIGNED, TAMPERED, TEST_KEY};
    use elgin_application::ports::ReleaseAsset;

    const URL: &str = "https://github.com/BrunoNC22/elgin-tray/releases/download/v9.9.9/pacote";

    /// Guarda o conteúdo do executável que seria instalado.
    #[derive(Debug, Default)]
    struct SpyReplacer {
        installed: Option<Vec<u8>>,
    }

    impl ExecutableReplacer for SpyReplacer {
        fn replace(&mut self, new_executable: &Path) -> io::Result<()> {
            self.installed = Some(std::fs::read(new_executable)?);
            Ok(())
        }
    }

    fn release() -> Release {
        Release {
            version: "9.9.9".parse().unwrap(),
            assets: vec![ReleaseAsset {
                name: NAME.to_owned(),
                url: URL.to_owned(),
            }],
        }
    }

    fn installer(package: &[u8]) -> ReleaseInstaller<FakeHttp, SpyReplacer> {
        let mut http = FakeHttp::default();
        http.respond(URL, 200, package);
        let staging = std::env::temp_dir().join(format!(
            "elgin-tray-teste-{}-{}",
            std::process::id(),
            package.len()
        ));
        ReleaseInstaller::new(
            http,
            SpyReplacer::default(),
            WINDOWS_X86_64,
            *TEST_KEY,
            staging,
        )
    }

    #[tokio::test]
    async fn installs_the_executable_of_a_signed_package() {
        let mut installer = installer(SIGNED);

        installer.install(&release()).await.unwrap();

        assert_eq!(installer.http.requested, [URL]);
        assert_eq!(installer.replacer.installed.as_deref(), Some(EXECUTABLE));
        let staged = installer.staging_dir.join("elgin-tray-9.9.9.new");
        assert!(!staged.exists(), "o arquivo temporário é apagado");
        let _ = std::fs::remove_dir_all(&installer.staging_dir);
    }

    #[tokio::test]
    async fn a_tampered_package_is_never_installed() {
        let mut installer = installer(TAMPERED);

        let result = installer.install(&release()).await;

        assert!(
            matches!(result, Err(UpdateError::Rejected(reason)) if reason.contains("assinatura"))
        );
        assert_eq!(installer.replacer.installed, None);
    }

    #[tokio::test]
    async fn download_failures_are_temporary() {
        let mut offline = installer(SIGNED);
        offline.http = FakeHttp::default();
        assert!(matches!(
            offline.install(&release()).await,
            Err(UpdateError::Network(_))
        ));

        let mut not_found = installer(SIGNED);
        not_found.http.respond(URL, 404, "Not Found");
        assert!(matches!(
            not_found.install(&release()).await,
            Err(UpdateError::Network(_))
        ));
        assert_eq!(not_found.replacer.installed, None);
    }

    #[tokio::test]
    async fn a_release_without_this_package_is_rejected() {
        let mut installer = installer(SIGNED);
        let mut release = release();
        release.assets.clear();

        assert!(matches!(
            installer.install(&release).await,
            Err(UpdateError::Rejected(_))
        ));
        assert!(installer.http.requested.is_empty());
    }
}
