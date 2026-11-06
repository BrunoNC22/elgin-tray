//! Verificação de atualizações: compara a última release com a versão em uso e, conforme a política
//! do sistema, instala sozinho (Windows) ou só avisa (Arch/AUR).

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::config::Settings;
use crate::ports::{Clock, ConfigRepository, Release, UpdateError, UpdateInstaller, UpdateSource};
use crate::tray::UpdatePolicy;

/// Espera depois de iniciar antes da primeira verificação (não atrasa a conexão com a lâmpada).
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
/// Intervalo entre verificações.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Versão `X.Y.Z` (as tags de release são `vX.Y.Z`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidVersion(pub String);

impl fmt::Display for InvalidVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "versão inválida \"{}\" (esperado X.Y.Z)", self.0)
    }
}

impl std::error::Error for InvalidVersion {}

impl FromStr for Version {
    type Err = InvalidVersion;

    /// Aceita `1.2.3` e `v1.2.3`. Pré-versões (`1.2.3-rc1`) não são releases: recusadas.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidVersion(text.to_owned());
        let digits = text.trim().strip_prefix('v').unwrap_or(text.trim());
        let mut parts = digits.split('.').map(|part| {
            (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
        });
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) => Ok(Self {
                major,
                minor,
                patch,
            }),
            _ => Err(invalid()),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// O resultado de uma verificação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// Já está na última versão.
    UpToDate,
    /// Há versão nova (política "só avisar"). `first_notice`: ainda não avisada ao usuário.
    Available {
        version: Version,
        first_notice: bool,
    },
    /// A versão nova foi instalada: o daemon precisa reiniciar.
    Installed(Version),
    /// Não deu para verificar ou instalar (rede, assinatura...). Só vai para o log nas
    /// verificações automáticas.
    Failed(UpdateError),
}

/// Verifica atualizações ao iniciar (depois de [`FIRST_CHECK_DELAY`]) e a cada [`CHECK_INTERVAL`].
///
/// Sem loop próprio: o daemon chama [`check`](Self::check) quando chega [`next_check_at`]
/// (ou quando o usuário pede pelo menu).
///
/// [`next_check_at`]: Self::next_check_at
#[derive(Debug)]
pub struct UpdateChecker<S, I, K> {
    source: S,
    installer: I,
    clock: K,
    policy: UpdatePolicy,
    current: Version,
    next_check_at: Instant,
    available: Option<Version>,
}

impl<S, I, K> UpdateChecker<S, I, K>
where
    S: UpdateSource,
    I: UpdateInstaller,
    K: Clock,
{
    pub fn new(source: S, installer: I, clock: K, policy: UpdatePolicy, current: Version) -> Self {
        let next_check_at = clock.now() + FIRST_CHECK_DELAY;
        Self {
            source,
            installer,
            clock,
            policy,
            current,
            next_check_at,
            available: None,
        }
    }

    pub fn current(&self) -> Version {
        self.current
    }

    pub fn next_check_at(&self) -> Instant {
        self.next_check_at
    }

    /// Versão nova conhecida e não instalada (para o menu da bandeja).
    pub fn available(&self) -> Option<Version> {
        self.available
    }

    /// Consulta a última release e age conforme a política. A próxima verificação fica para daqui
    /// a [`CHECK_INTERVAL`].
    pub async fn check<R: ConfigRepository>(
        &mut self,
        settings: &mut Settings<R>,
    ) -> UpdateOutcome {
        self.next_check_at = self.clock.now() + CHECK_INTERVAL;
        let release = match self.source.latest_release().await {
            Ok(release) => release,
            Err(error) => return UpdateOutcome::Failed(error),
        };
        if release.version <= self.current {
            self.available = None;
            return UpdateOutcome::UpToDate;
        }
        match self.policy {
            UpdatePolicy::AutoInstall => self.install(&release).await,
            UpdatePolicy::NotifyOnly => {
                self.available = Some(release.version);
                let version = release.version.to_string();
                let first_notice =
                    settings.config().updates.last_notified_version.as_deref() != Some(&version);
                // Se não der para gravar, o pior caso é avisar de novo na próxima verificação.
                if first_notice {
                    let _ = settings.mark_version_notified(&version);
                }
                UpdateOutcome::Available {
                    version: release.version,
                    first_notice,
                }
            }
        }
    }

    async fn install(&mut self, release: &Release) -> UpdateOutcome {
        match self.installer.install(release).await {
            Ok(()) => {
                self.available = None;
                UpdateOutcome::Installed(release.version)
            }
            Err(error) => {
                // Fica disponível no menu ("Instalar vX") para tentar de novo.
                self.available = Some(release.version);
                UpdateOutcome::Failed(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::testing::{FakeConfigRepository, ManualClock, SpyInstaller, StubUpdateSource};

    fn version(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn parses_release_versions() {
        assert_eq!(
            version("v1.2.3"),
            Version {
                major: 1,
                minor: 2,
                patch: 3
            }
        );
        assert_eq!(version("0.10.0").to_string(), "0.10.0");
        for invalid in [
            "",
            "1.2",
            "1.2.3.4",
            "v1.2.x",
            "1.2.3-rc1",
            "1..3",
            "+1.2.3",
        ] {
            assert!(invalid.parse::<Version>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(version("0.10.0") > version("0.9.9"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("1.0.1") > version("1.0.0"));
        assert_eq!(version("v1.0.0"), version("1.0.0"));
    }

    struct Fixture {
        checker: UpdateChecker<StubUpdateSource, SpyInstaller, ManualClock>,
        source: StubUpdateSource,
        installer: SpyInstaller,
        clock: ManualClock,
        settings: Settings<FakeConfigRepository>,
        repository: FakeConfigRepository,
    }

    fn fixture(policy: UpdatePolicy) -> Fixture {
        let source = StubUpdateSource::new();
        let installer = SpyInstaller::new();
        let clock = ManualClock::new();
        let repository = FakeConfigRepository::with(AppConfig::default());
        Fixture {
            checker: UpdateChecker::new(
                source.clone(),
                installer.clone(),
                clock.clone(),
                policy,
                version("1.0.0"),
            ),
            source,
            installer,
            clock,
            settings: Settings::load(repository.clone()).unwrap(),
            repository,
        }
    }

    #[tokio::test]
    async fn checks_shortly_after_starting_and_then_every_hour() {
        let mut f = fixture(UpdatePolicy::AutoInstall);
        let start = f.clock.now();
        assert_eq!(f.checker.next_check_at(), start + FIRST_CHECK_DELAY);

        f.clock.advance(FIRST_CHECK_DELAY);
        f.checker.check(&mut f.settings).await;

        assert_eq!(f.source.calls(), 1);
        assert_eq!(
            f.checker.next_check_at(),
            start + FIRST_CHECK_DELAY + CHECK_INTERVAL
        );
    }

    #[tokio::test]
    async fn the_same_or_an_older_version_is_up_to_date() {
        let mut f = fixture(UpdatePolicy::AutoInstall);
        for latest in ["1.0.0", "0.9.0"] {
            f.source.latest(latest);

            assert_eq!(
                f.checker.check(&mut f.settings).await,
                UpdateOutcome::UpToDate
            );
        }
        assert!(f.installer.installed().is_empty());
        assert_eq!(f.checker.available(), None);
    }

    #[tokio::test]
    async fn auto_install_installs_the_new_version() {
        let mut f = fixture(UpdatePolicy::AutoInstall);
        f.source.latest("1.1.0");

        assert_eq!(
            f.checker.check(&mut f.settings).await,
            UpdateOutcome::Installed(version("1.1.0"))
        );
        assert_eq!(f.installer.installed(), [version("1.1.0")]);
        assert_eq!(f.checker.available(), None);
    }

    #[tokio::test]
    async fn a_failed_install_is_reported_and_stays_available() {
        let mut f = fixture(UpdatePolicy::AutoInstall);
        f.source.latest("1.1.0");
        let refused = UpdateError::Rejected("assinatura inválida".to_owned());
        f.installer.fail_with(refused.clone());

        assert_eq!(
            f.checker.check(&mut f.settings).await,
            UpdateOutcome::Failed(refused)
        );
        assert_eq!(f.checker.available(), Some(version("1.1.0")));
    }

    #[tokio::test]
    async fn notify_only_never_installs_and_notifies_once_per_version() {
        let mut f = fixture(UpdatePolicy::NotifyOnly);
        f.source.latest("1.1.0");

        assert_eq!(
            f.checker.check(&mut f.settings).await,
            UpdateOutcome::Available {
                version: version("1.1.0"),
                first_notice: true
            }
        );
        assert_eq!(
            f.repository.saved().unwrap().updates.last_notified_version,
            Some("1.1.0".to_owned())
        );
        assert_eq!(
            f.checker.check(&mut f.settings).await,
            UpdateOutcome::Available {
                version: version("1.1.0"),
                first_notice: false
            }
        );

        f.source.latest("1.2.0");
        assert!(matches!(
            f.checker.check(&mut f.settings).await,
            UpdateOutcome::Available {
                first_notice: true,
                ..
            }
        ));
        assert!(f.installer.installed().is_empty());
        assert_eq!(f.checker.available(), Some(version("1.2.0")));
    }

    #[tokio::test]
    async fn network_failures_are_reported_and_retried_in_an_hour() {
        let mut f = fixture(UpdatePolicy::NotifyOnly);
        let offline = UpdateError::Network("timeout".to_owned());
        f.source.returns(Err(offline.clone()));

        assert_eq!(
            f.checker.check(&mut f.settings).await,
            UpdateOutcome::Failed(offline)
        );
        assert_eq!(f.checker.next_check_at(), f.clock.now() + CHECK_INTERVAL);
        assert_eq!(
            f.repository.saved().unwrap().updates.last_notified_version,
            None
        );
    }
}
