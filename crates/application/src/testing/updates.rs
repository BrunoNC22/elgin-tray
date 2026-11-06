use std::cell::RefCell;
use std::rc::Rc;

use crate::ports::{Release, UpdateError, UpdateInstaller, UpdateSource};
use crate::updates::Version;

/// Fonte de versões roteirizada: devolve sempre a última resposta configurada e conta as consultas.
#[derive(Debug, Clone)]
pub struct StubUpdateSource {
    inner: Rc<RefCell<SourceInner>>,
}

#[derive(Debug)]
struct SourceInner {
    response: Result<Release, UpdateError>,
    calls: usize,
}

impl StubUpdateSource {
    /// Começa respondendo "sem conexão".
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(SourceInner {
                response: Err(UpdateError::Network("roteiro vazio".to_owned())),
                calls: 0,
            })),
        }
    }

    pub fn returns(&self, response: Result<Release, UpdateError>) {
        self.inner.borrow_mut().response = response;
    }

    /// Atalho: a última release é `version`, sem arquivos.
    pub fn latest(&self, version: &str) {
        self.returns(Ok(Release {
            version: version.parse().unwrap(),
            assets: Vec::new(),
        }));
    }

    pub fn calls(&self) -> usize {
        self.inner.borrow().calls
    }
}

impl Default for StubUpdateSource {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateSource for StubUpdateSource {
    async fn latest_release(&mut self) -> Result<Release, UpdateError> {
        let mut inner = self.inner.borrow_mut();
        inner.calls += 1;
        inner.response.clone()
    }
}

/// Instalador em memória: guarda as versões instaladas e pode falhar.
#[derive(Debug, Clone, Default)]
pub struct SpyInstaller {
    inner: Rc<RefCell<InstallerInner>>,
}

#[derive(Debug, Default)]
struct InstallerInner {
    installed: Vec<Version>,
    failure: Option<UpdateError>,
}

impl SpyInstaller {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn installed(&self) -> Vec<Version> {
        self.inner.borrow().installed.clone()
    }

    /// As próximas instalações falham com `error`.
    pub fn fail_with(&self, error: UpdateError) {
        self.inner.borrow_mut().failure = Some(error);
    }
}

impl UpdateInstaller for SpyInstaller {
    async fn install(&mut self, release: &Release) -> Result<(), UpdateError> {
        let mut inner = self.inner.borrow_mut();
        if let Some(error) = inner.failure.clone() {
            return Err(error);
        }
        inner.installed.push(release.version);
        Ok(())
    }
}
