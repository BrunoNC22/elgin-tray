use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::config::AppConfig;
use crate::ports::{ConfigError, ConfigRepository};

/// Repositório de configuração em memória, com falhas roteirizadas e contagem de gravações.
#[derive(Debug, Clone, Default)]
pub struct FakeConfigRepository {
    inner: Rc<RefCell<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    saved: Option<AppConfig>,
    saves: usize,
    failures: VecDeque<ConfigError>,
}

impl FakeConfigRepository {
    /// Sem configuração salva.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Com `config` já salva.
    pub fn with(config: AppConfig) -> Self {
        let repository = Self::default();
        repository.inner.borrow_mut().saved = Some(config);
        repository
    }

    /// A próxima chamada (`load` ou `save`) falha com `error`.
    pub fn fail_next(&self, error: ConfigError) {
        self.inner.borrow_mut().failures.push_back(error);
    }

    pub fn saved(&self) -> Option<AppConfig> {
        self.inner.borrow().saved.clone()
    }

    /// Quantas vezes `save` gravou com sucesso.
    pub fn save_count(&self) -> usize {
        self.inner.borrow().saves
    }

    fn take_failure(&self) -> Result<(), ConfigError> {
        match self.inner.borrow_mut().failures.pop_front() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl ConfigRepository for FakeConfigRepository {
    fn load(&self) -> Result<Option<AppConfig>, ConfigError> {
        self.take_failure()?;
        Ok(self.inner.borrow().saved.clone())
    }

    fn save(&mut self, config: &AppConfig) -> Result<(), ConfigError> {
        self.take_failure()?;
        let mut inner = self.inner.borrow_mut();
        inner.saved = Some(config.clone());
        inner.saves += 1;
        Ok(())
    }
}
