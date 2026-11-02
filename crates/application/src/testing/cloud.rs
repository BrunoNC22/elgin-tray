use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::config::CloudSession;
use crate::ports::{CloudAccount, CloudDevices, CloudError, LoginPoll, QrLogin};

/// Conta na nuvem roteirizada: devolve os resultados enfileirados em ordem e registra os pedidos.
#[derive(Debug, Clone, Default)]
pub struct StubCloudAccount {
    inner: Rc<RefCell<CloudInner>>,
}

#[derive(Debug, Default)]
struct CloudInner {
    starts: VecDeque<Result<QrLogin, CloudError>>,
    polls: VecDeque<Result<LoginPoll, CloudError>>,
    listings: VecDeque<Result<CloudDevices, CloudError>>,
    listed_sessions: Vec<CloudSession>,
    user_codes: Vec<String>,
    poll_count: usize,
}

impl StubCloudAccount {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_returns(&self, result: Result<QrLogin, CloudError>) {
        self.inner.borrow_mut().starts.push_back(result);
    }

    pub fn poll_returns(&self, result: Result<LoginPoll, CloudError>) {
        self.inner.borrow_mut().polls.push_back(result);
    }

    pub fn list_returns(&self, result: Result<CloudDevices, CloudError>) {
        self.inner.borrow_mut().listings.push_back(result);
    }

    /// Sessões usadas em `list_devices`, em ordem.
    pub fn listed_sessions(&self) -> Vec<CloudSession> {
        self.inner.borrow().listed_sessions.clone()
    }

    /// User codes usados em `start_qr_login`.
    pub fn user_codes(&self) -> Vec<String> {
        self.inner.borrow().user_codes.clone()
    }

    pub fn poll_count(&self) -> usize {
        self.inner.borrow().poll_count
    }
}

impl CloudAccount for StubCloudAccount {
    async fn start_qr_login(&mut self, user_code: &str) -> Result<QrLogin, CloudError> {
        let mut inner = self.inner.borrow_mut();
        inner.user_codes.push(user_code.to_owned());
        inner
            .starts
            .pop_front()
            .expect("nenhum resultado roteirizado para start_qr_login")
    }

    async fn poll_login(&mut self, _login: &QrLogin) -> Result<LoginPoll, CloudError> {
        let mut inner = self.inner.borrow_mut();
        inner.poll_count += 1;
        // Sem nada roteirizado, o login continua pendente.
        inner.polls.pop_front().unwrap_or(Ok(LoginPoll::Pending))
    }

    async fn list_devices(&mut self, session: &CloudSession) -> Result<CloudDevices, CloudError> {
        let mut inner = self.inner.borrow_mut();
        inner.listed_sessions.push(session.clone());
        inner
            .listings
            .pop_front()
            .expect("nenhum resultado roteirizado para list_devices")
    }
}
