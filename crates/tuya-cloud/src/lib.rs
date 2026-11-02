//! Adaptador: login por QR, lista de dispositivos e local key pela API de nuvem da Tuya.
//!
//! Usa a mesma API do `tuya-device-sharing-sdk` (a integração Tuya do Home Assistant), que não exige
//! conta de desenvolvedor: o usuário só confirma o login no app Smart Life.

// Como na aplicação: tudo roda numa thread só, então as futures não precisam ser `Send`.
#![allow(async_fn_in_trait)]

mod http;
mod login;
mod session;

pub use http::{HttpClient, HttpRequest, HttpResponse, Method, UreqClient};

use elgin_application::ports::{CloudAccount, CloudError, LoginPoll, QrLogin};

/// A conta do usuário na nuvem da Tuya (implementa a porta `CloudAccount`).
#[derive(Debug, Clone, Default)]
pub struct TuyaCloud<H = UreqClient> {
    http: H,
}

impl<H: HttpClient> TuyaCloud<H> {
    pub fn new(http: H) -> Self {
        Self { http }
    }
}

impl<H: HttpClient> CloudAccount for TuyaCloud<H> {
    async fn start_qr_login(&mut self, user_code: &str) -> Result<QrLogin, CloudError> {
        login::start(&mut self.http, user_code).await
    }

    async fn poll_login(&mut self, login: &QrLogin) -> Result<LoginPoll, CloudError> {
        login::poll(&mut self.http, login).await
    }
}
