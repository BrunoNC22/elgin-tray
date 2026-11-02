//! Login por QR code do Smart Life, o mesmo da integração Tuya do Home Assistant.
//!
//! Endpoints (do `tuya_sharing/user.py` do `tuya-device-sharing-sdk`):
//! - `POST https://apigw.iotbing.com/v1.0/m/life/home-assistant/qrcode/tokens?clientid=..&usercode=..&schema=haauthorize`
//!   → `{"success": true, "result": {"qrcode": "<token>"}, "t": <ms>}`
//! - `GET  https://apigw.iotbing.com/v1.0/m/life/home-assistant/qrcode/tokens/<token>?clientid=..&usercode=..`
//!   → enquanto não confirmado, `success: false`; confirmado, `result` com `terminal_id`,
//!   `endpoint`, `uid`, `access_token`, `refresh_token`, `expire_time` (s) e `t` (ms) na raiz.
//!
//! O QR code desenhado é `tuyaSmart--qrLogin?token=<token>` (o Smart Life e o Tuya Smart leem).

use elgin_application::ports::{CloudError, LoginPoll, QrLogin};
use serde_json::Value;

use crate::http::{HttpClient, HttpRequest, Method};
use crate::session::SessionData;

/// Registro público do Home Assistant na plataforma da Tuya (o mesmo usado pelo tuya-local-key).
/// Não é segredo: a autorização de verdade é a confirmação no celular. A Tuya pode mudá-lo.
pub(crate) const CLIENT_ID: &str = "HA_3y9q4ak7g4ephrvke";
const SCHEMA: &str = "haauthorize";
const LOGIN_HOST: &str = "https://apigw.iotbing.com";
const QR_PREFIX: &str = "tuyaSmart--qrLogin?token=";

/// Pede o token do QR code.
pub(crate) async fn start<H: HttpClient>(
    http: &mut H,
    user_code: &str,
) -> Result<QrLogin, CloudError> {
    // O user code vai na URL: só letras e números (é o formato do Smart Life), sem precisar escapar.
    if user_code.is_empty() || !user_code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(CloudError::Rejected(
            "o código de usuário deve ter só letras e números".to_owned(),
        ));
    }
    let url = format!(
        "{LOGIN_HOST}/v1.0/m/life/home-assistant/qrcode/tokens?clientid={CLIENT_ID}&usercode={user_code}&schema={SCHEMA}"
    );
    let json = call(http, Method::Post, url).await?;
    if json["success"] != true {
        return Err(CloudError::Rejected(server_message(&json)));
    }
    let token = json["result"]["qrcode"]
        .as_str()
        .ok_or_else(|| CloudError::Protocol("resposta sem o token do QR code".to_owned()))?;
    Ok(QrLogin {
        user_code: user_code.to_owned(),
        token: token.to_owned(),
        qr_content: format!("{QR_PREFIX}{token}"),
    })
}

/// Consulta se o login foi confirmado.
pub(crate) async fn poll<H: HttpClient>(
    http: &mut H,
    login: &QrLogin,
) -> Result<LoginPoll, CloudError> {
    let url = format!(
        "{LOGIN_HOST}/v1.0/m/life/home-assistant/qrcode/tokens/{}?clientid={CLIENT_ID}&usercode={}",
        login.token, login.user_code
    );
    let json = call(http, Method::Get, url).await?;
    // Enquanto o QR não é confirmado no celular a API responde `success: false`.
    if json["success"] != true {
        return Ok(LoginPoll::Pending);
    }
    let result = &json["result"];
    let text = |field: &str| {
        result[field]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| CloudError::Protocol(format!("login confirmado sem o campo {field}")))
    };
    let issued_at_ms = json["t"].as_u64().unwrap_or(0);
    let lifetime_s = result["expire_time"].as_u64().unwrap_or(0);
    let session = SessionData {
        user_code: login.user_code.clone(),
        terminal_id: text("terminal_id")?,
        endpoint: text("endpoint")?,
        uid: text("uid")?,
        access_token: text("access_token")?,
        refresh_token: text("refresh_token")?,
        expires_at_ms: issued_at_ms + lifetime_s * 1000,
    };
    Ok(LoginPoll::Done(session.to_cloud_session()))
}

/// Faz a requisição (sem corpo) e lê o JSON da resposta.
async fn call<H: HttpClient>(
    http: &mut H,
    method: Method,
    url: String,
) -> Result<Value, CloudError> {
    let response = http
        .send(HttpRequest {
            method,
            url,
            headers: Vec::new(),
            body: None,
        })
        .await
        .map_err(CloudError::Network)?;
    serde_json::from_str(&response.body).map_err(|_| {
        CloudError::Protocol(format!("HTTP {}: a resposta não é JSON", response.status))
    })
}

/// A mensagem de erro que o servidor mandou.
pub(crate) fn server_message(json: &Value) -> String {
    json["msg"]
        .as_str()
        .or_else(|| json["code"].as_str())
        .unwrap_or("erro desconhecido")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::FakeHttp;

    const QR_RESPONSE: &str =
        r#"{"result":{"qrcode":"AYtoken123"},"success":true,"t":1700000000000,"tid":"x"}"#;
    const PENDING_RESPONSE: &str =
        r#"{"code":"1010","msg":"token invalid","success":false,"t":1700000001000}"#;
    const DONE_RESPONSE: &str = r#"{"result":{"access_token":"acesso","endpoint":"https://apigw.tuyaus.com","expire_time":7200,"refresh_token":"renovacao","terminal_id":"terminal","uid":"az123"},"success":true,"t":1700000000000}"#;

    fn login() -> QrLogin {
        QrLogin {
            user_code: "Abc1234".to_owned(),
            token: "AYtoken123".to_owned(),
            qr_content: "tuyaSmart--qrLogin?token=AYtoken123".to_owned(),
        }
    }

    #[tokio::test]
    async fn start_asks_for_a_token_and_builds_the_qr_content() {
        let mut http = FakeHttp::default();
        http.respond(200, QR_RESPONSE);

        let result = start(&mut http, "Abc1234").await;

        assert_eq!(result, Ok(login()));
        let request = &http.requests()[0];
        assert_eq!(request.method, Method::Post);
        assert_eq!(
            request.url,
            "https://apigw.iotbing.com/v1.0/m/life/home-assistant/qrcode/tokens?clientid=HA_3y9q4ak7g4ephrvke&usercode=Abc1234&schema=haauthorize"
        );
    }

    #[tokio::test]
    async fn an_invalid_user_code_is_rejected_with_the_server_message() {
        let mut http = FakeHttp::default();
        http.respond(
            200,
            r#"{"code":"1106","msg":"user code invalid","success":false}"#,
        );

        let result = start(&mut http, "errado1").await;

        assert_eq!(
            result,
            Err(CloudError::Rejected("user code invalid".to_owned()))
        );
    }

    #[tokio::test]
    async fn a_user_code_with_strange_characters_is_refused_without_calling_the_server() {
        let mut http = FakeHttp::default();

        let result = start(&mut http, "abc&x=1").await;

        assert!(matches!(result, Err(CloudError::Rejected(_))));
        assert!(http.requests().is_empty());
    }

    #[tokio::test]
    async fn network_and_garbage_responses_are_reported() {
        let mut http = FakeHttp::default();
        http.fail("sem internet");
        http.respond(502, "<html>bad gateway</html>");

        assert_eq!(
            start(&mut http, "Abc1234").await,
            Err(CloudError::Network("sem internet".to_owned()))
        );
        assert!(matches!(
            start(&mut http, "Abc1234").await,
            Err(CloudError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn poll_is_pending_until_confirmed() {
        let mut http = FakeHttp::default();
        http.respond(200, PENDING_RESPONSE);

        assert_eq!(poll(&mut http, &login()).await, Ok(LoginPoll::Pending));
        assert_eq!(
            http.requests()[0].url,
            "https://apigw.iotbing.com/v1.0/m/life/home-assistant/qrcode/tokens/AYtoken123?clientid=HA_3y9q4ak7g4ephrvke&usercode=Abc1234"
        );
        assert_eq!(http.requests()[0].method, Method::Get);
    }

    #[tokio::test]
    async fn a_confirmed_login_gives_the_session() {
        let mut http = FakeHttp::default();
        http.respond(200, DONE_RESPONSE);

        let result = poll(&mut http, &login()).await.unwrap();

        let LoginPoll::Done(session) = result else {
            panic!("esperava Done, veio {result:?}");
        };
        assert_eq!(
            SessionData::from_cloud_session(&session),
            Ok(SessionData {
                user_code: "Abc1234".to_owned(),
                terminal_id: "terminal".to_owned(),
                endpoint: "https://apigw.tuyaus.com".to_owned(),
                uid: "az123".to_owned(),
                access_token: "acesso".to_owned(),
                refresh_token: "renovacao".to_owned(),
                expires_at_ms: 1_700_000_000_000 + 7_200_000,
            })
        );
    }

    #[tokio::test]
    async fn a_confirmed_login_without_the_needed_fields_is_a_protocol_error() {
        let mut http = FakeHttp::default();
        http.respond(200, r#"{"result":{"uid":"az123"},"success":true,"t":1}"#);

        assert!(matches!(
            poll(&mut http, &login()).await,
            Err(CloudError::Protocol(_))
        ));
    }
}
