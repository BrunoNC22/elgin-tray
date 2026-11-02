//! Adaptador: login por QR, lista de dispositivos e local key pela API de nuvem da Tuya.
//!
//! Usa a mesma API do `tuya-device-sharing-sdk` (a integração Tuya do Home Assistant), que não exige
//! conta de desenvolvedor: o usuário só confirma o login no app Smart Life.

// Como na aplicação: tudo roda numa thread só, então as futures não precisam ser `Send`.
#![allow(async_fn_in_trait)]

mod crypto;
mod devices;
mod env;
mod http;
mod login;
mod session;

pub use env::{CloudEnv, SystemEnv};
pub use http::{HttpClient, HttpRequest, HttpResponse, Method, UreqClient};

use elgin_application::config::CloudSession;
use elgin_application::ports::{CloudAccount, CloudDevices, CloudError, LoginPoll, QrLogin};

use crate::session::SessionData;

/// A conta do usuário na nuvem da Tuya (implementa a porta `CloudAccount`).
#[derive(Debug, Clone, Default)]
pub struct TuyaCloud<H = UreqClient, E = SystemEnv> {
    http: H,
    env: E,
}

impl<H: HttpClient, E: CloudEnv> TuyaCloud<H, E> {
    pub fn new(http: H, env: E) -> Self {
        Self { http, env }
    }
}

impl<H: HttpClient, E: CloudEnv> CloudAccount for TuyaCloud<H, E> {
    async fn start_qr_login(&mut self, user_code: &str) -> Result<QrLogin, CloudError> {
        login::start(&mut self.http, user_code).await
    }

    async fn poll_login(&mut self, login: &QrLogin) -> Result<LoginPoll, CloudError> {
        login::poll(&mut self.http, login).await
    }

    async fn list_devices(&mut self, session: &CloudSession) -> Result<CloudDevices, CloudError> {
        let data = SessionData::from_cloud_session(session)?;
        let (devices, data) = devices::list_devices(&mut self.http, &mut self.env, data).await?;
        Ok(CloudDevices {
            devices,
            session: data.to_cloud_session(),
        })
    }
}
