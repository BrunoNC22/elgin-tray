//! Chamadas assinadas da API: renovação da sessão, casas e dispositivos.
//!
//! Endpoints (do `tuya-device-sharing-sdk`):
//! - `GET /v1.0/m/token/<refresh_token>` → `{expireTime, uid, accessToken, refreshToken}` + `t` na raiz;
//! - `GET /v1.0/m/life/users/homes` → `[{ownerId, name}]`;
//! - `GET /v1.0/m/life/ha/home/devices?homeId=..` → `[{id, name, local_key, category, product_name, ...}]`.

use elgin_application::credentials::LocalKey;
use elgin_application::ports::{CloudDevice, CloudError};
use serde_json::{Value, json};

use crate::crypto::RequestKeys;
use crate::env::CloudEnv;
use crate::http::{HttpClient, HttpRequest, Method};
use crate::login::{CLIENT_ID, server_message};
use crate::session::SessionData;

/// Renova o token se faltar menos que isto para expirar (como o SDK).
const REFRESH_MARGIN_MS: u64 = 60_000;

/// Lista os dispositivos de todas as casas da conta. Devolve também a sessão (renovada, se foi preciso).
pub(crate) async fn list_devices<H: HttpClient, E: CloudEnv>(
    http: &mut H,
    env: &mut E,
    session: SessionData,
) -> Result<(Vec<CloudDevice>, SessionData), CloudError> {
    let session = if env.now_ms() + REFRESH_MARGIN_MS >= session.expires_at_ms {
        renew(http, env, session).await?
    } else {
        session
    };

    let homes = call(http, env, &session, "/v1.0/m/life/users/homes", None).await?;
    let mut devices: Vec<CloudDevice> = Vec::new();
    for home in homes
        .result
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // O id da casa vem como número ou texto, conforme a conta.
        let home_id = match &home["ownerId"] {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let params = json!({ "homeId": home_id });
        let listed = call(
            http,
            env,
            &session,
            "/v1.0/m/life/ha/home/devices",
            Some(&params),
        )
        .await?;
        for item in listed
            .result
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let Some(device) = parse_device(item) else {
                continue;
            };
            // Um dispositivo compartilhado aparece em mais de uma casa.
            if !devices.iter().any(|known| known.id == device.id) {
                devices.push(device);
            }
        }
    }
    Ok((devices, session))
}

fn parse_device(item: &Value) -> Option<CloudDevice> {
    let text = |field: &str| item[field].as_str().unwrap_or_default().to_owned();
    let id = item["id"].as_str()?.to_owned();
    Some(CloudDevice {
        id,
        name: text("name"),
        local_key: LocalKey::new(text("local_key")),
        category: text("category"),
        product_name: text("product_name"),
    })
}

/// Troca o refresh token por tokens novos. Qualquer falha quer dizer que a sessão não vale mais.
async fn renew<H: HttpClient, E: CloudEnv>(
    http: &mut H,
    env: &mut E,
    session: SessionData,
) -> Result<SessionData, CloudError> {
    let path = format!("/v1.0/m/token/{}", session.refresh_token);
    let response = match call(http, env, &session, &path, None).await {
        Ok(response) => response,
        Err(CloudError::Network(detail)) => return Err(CloudError::Network(detail)),
        Err(_) => return Err(CloudError::SessionExpired),
    };
    let result = &response.result;
    let text = |field: &str| {
        result[field]
            .as_str()
            .map(str::to_owned)
            .ok_or(CloudError::SessionExpired)
    };
    Ok(SessionData {
        access_token: text("accessToken")?,
        refresh_token: text("refreshToken")?,
        uid: text("uid").unwrap_or(session.uid.clone()),
        expires_at_ms: response.t + result["expireTime"].as_u64().unwrap_or(0) * 1000,
        ..session
    })
}

/// Resposta de sucesso: o `result` já decifrado e o `t` (ms) do servidor.
struct ApiResponse {
    result: Value,
    t: u64,
}

/// Requisição GET assinada; `params` vão cifrados na query.
async fn call<H: HttpClient, E: CloudEnv>(
    http: &mut H,
    env: &mut E,
    session: &SessionData,
    path: &str,
    params: Option<&Value>,
) -> Result<ApiResponse, CloudError> {
    let request_id = env.request_id();
    let keys = RequestKeys::derive(&request_id, &session.refresh_token);
    let query = params.map_or_else(String::new, |params| {
        keys.encrypt(&params.to_string(), &env.nonce())
    });
    let time = env.now_ms().to_string();
    let headers = [
        ("X-appKey", CLIENT_ID),
        ("X-requestId", request_id.as_str()),
        ("X-sid", ""),
        ("X-time", time.as_str()),
        ("X-token", session.access_token.as_str()),
    ];
    let sign = keys.sign(&headers, &query, "");
    let mut url = format!("{}{path}", session.endpoint);
    if !query.is_empty() {
        url.push_str("?encdata=");
        url.push_str(&percent_encode(&query));
    }
    let mut request_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    request_headers.push(("X-sign".to_owned(), sign));

    let response = http
        .send(HttpRequest {
            method: Method::Get,
            url,
            headers: request_headers,
            body: None,
        })
        .await
        .map_err(CloudError::Network)?;
    if !(200..300).contains(&response.status) {
        return Err(CloudError::Protocol(format!("HTTP {}", response.status)));
    }
    let json: Value = serde_json::from_str(&response.body)
        .map_err(|_| CloudError::Protocol("a resposta não é JSON".to_owned()))?;
    if json["success"] != true {
        let message = server_message(&json);
        return Err(if message.to_lowercase().contains("token") {
            CloudError::SessionExpired
        } else {
            CloudError::Rejected(message)
        });
    }
    let result = match &json["result"] {
        Value::String(encrypted) => {
            let plain = keys.decrypt(encrypted).ok_or_else(|| {
                CloudError::Protocol("não foi possível decifrar a resposta".to_owned())
            })?;
            serde_json::from_str(&plain).unwrap_or(Value::String(plain))
        }
        other => other.clone(),
    };
    Ok(ApiResponse {
        result,
        t: json["t"].as_u64().unwrap_or_else(|| env.now_ms()),
    })
}

/// Codifica para a URL tudo que não for letra, número ou `-_.~`.
pub(crate) fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::FakeHttp;
    use std::collections::VecDeque;

    /// Ambiente roteirizado: ids de requisição em ordem, nonce e relógio fixos.
    struct FixedEnv {
        ids: VecDeque<String>,
        now_ms: u64,
    }

    impl FixedEnv {
        fn new(now_ms: u64) -> Self {
            Self {
                ids: (1..=20).map(|n| format!("req-{n}")).collect(),
                now_ms,
            }
        }
    }

    impl CloudEnv for FixedEnv {
        fn request_id(&mut self) -> String {
            self.ids.pop_front().unwrap()
        }
        fn nonce(&mut self) -> String {
            "ABCDEFGHJKMN".to_owned()
        }
        fn now_ms(&self) -> u64 {
            self.now_ms
        }
    }

    const NOW: u64 = 1_700_000_000_000;

    fn session(expires_at_ms: u64) -> SessionData {
        SessionData {
            user_code: "Abc1234".to_owned(),
            terminal_id: "terminal".to_owned(),
            endpoint: "https://apigw.tuyaus.com".to_owned(),
            uid: "az123".to_owned(),
            access_token: "acesso-velho".to_owned(),
            refresh_token: "renovacao-velha".to_owned(),
            expires_at_ms,
        }
    }

    /// Resposta de sucesso como o servidor manda, cifrada para a requisição `request_id`.
    fn ok(http: &FakeHttp, request_id: &str, refresh_token: &str, result: Value) {
        let keys = RequestKeys::derive(request_id, refresh_token);
        let encrypted = keys.encrypt_result(&result.to_string(), "ZYXWVUTSRQPN");
        http.respond(
            200,
            &json!({"success": true, "result": encrypted, "t": NOW + 5}).to_string(),
        );
    }

    fn homes() -> Value {
        json!([{"ownerId": 111, "name": "Casa"}, {"ownerId": "222", "name": "Praia"}])
    }

    fn lamp(id: &str, key: &str) -> Value {
        json!({"id": id, "name": "Lâmpada do quarto", "local_key": key, "category": "dj",
               "product_name": "10W Smart color", "online": true})
    }

    fn device(id: &str, key: &str) -> CloudDevice {
        CloudDevice {
            id: id.to_owned(),
            name: "Lâmpada do quarto".to_owned(),
            local_key: LocalKey::new(key),
            category: "dj".to_owned(),
            product_name: "10W Smart color".to_owned(),
        }
    }

    fn header<'a>(request: &'a HttpRequest, name: &str) -> &'a str {
        request
            .headers
            .iter()
            .find(|(header, _)| header == name)
            .map_or("", |(_, value)| value.as_str())
    }

    #[tokio::test]
    async fn lists_the_devices_of_every_home() {
        let mut http = FakeHttp::default();
        ok(&http, "req-1", "renovacao-velha", homes());
        ok(
            &http,
            "req-2",
            "renovacao-velha",
            json!([lamp("bf1", "chave-1")]),
        );
        ok(
            &http,
            "req-3",
            "renovacao-velha",
            json!([lamp("bf2", "chave-2")]),
        );

        let (devices, returned) =
            list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000))
                .await
                .unwrap();

        assert_eq!(
            devices,
            [device("bf1", "chave-1"), device("bf2", "chave-2")]
        );
        assert_eq!(returned, session(NOW + 3_600_000), "sessão válida não muda");
        let requests = http.requests();
        assert_eq!(
            requests[0].url,
            "https://apigw.tuyaus.com/v1.0/m/life/users/homes"
        );
        assert!(
            requests[1]
                .url
                .starts_with("https://apigw.tuyaus.com/v1.0/m/life/ha/home/devices?encdata=")
        );
    }

    #[tokio::test]
    async fn requests_are_signed_like_the_sdk() {
        let mut http = FakeHttp::default();
        ok(&http, "req-1", "renovacao-velha", json!([]));

        list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000))
            .await
            .unwrap();

        let request = &http.requests()[0];
        let keys = RequestKeys::derive("req-1", "renovacao-velha");
        let time = NOW.to_string();
        let expected = keys.sign(
            &[
                ("X-appKey", CLIENT_ID),
                ("X-requestId", "req-1"),
                ("X-sid", ""),
                ("X-time", &time),
                ("X-token", "acesso-velho"),
            ],
            "",
            "",
        );
        assert_eq!(request.method, Method::Get);
        assert_eq!(header(request, "X-appKey"), CLIENT_ID);
        assert_eq!(header(request, "X-requestId"), "req-1");
        assert_eq!(header(request, "X-time"), time);
        assert_eq!(header(request, "X-token"), "acesso-velho");
        assert_eq!(header(request, "X-sign"), expected);
    }

    #[tokio::test]
    async fn the_home_id_goes_encrypted_in_the_query() {
        let mut http = FakeHttp::default();
        ok(
            &http,
            "req-1",
            "renovacao-velha",
            json!([{"ownerId": 111, "name": "Casa"}]),
        );
        ok(&http, "req-2", "renovacao-velha", json!([]));

        list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000))
            .await
            .unwrap();

        let keys = RequestKeys::derive("req-2", "renovacao-velha");
        let encdata = keys.encrypt(r#"{"homeId":"111"}"#, "ABCDEFGHJKMN");
        let url = &http.requests()[1].url;
        assert!(
            url.ends_with(&format!("?encdata={}", percent_encode(&encdata))),
            "{url}"
        );
    }

    #[tokio::test]
    async fn a_device_in_two_homes_is_listed_once() {
        let mut http = FakeHttp::default();
        ok(&http, "req-1", "renovacao-velha", homes());
        ok(
            &http,
            "req-2",
            "renovacao-velha",
            json!([lamp("bf1", "chave-1")]),
        );
        ok(
            &http,
            "req-3",
            "renovacao-velha",
            json!([lamp("bf1", "chave-1")]),
        );

        let (devices, _) =
            list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000))
                .await
                .unwrap();

        assert_eq!(devices, [device("bf1", "chave-1")]);
    }

    #[tokio::test]
    async fn an_expiring_token_is_renewed_first() {
        let mut http = FakeHttp::default();
        ok(
            &http,
            "req-1",
            "renovacao-velha",
            json!({"expireTime": 7200, "uid": "az123",
            "accessToken": "acesso-novo", "refreshToken": "renovacao-nova"}),
        );
        ok(&http, "req-2", "renovacao-nova", json!([]));

        let (_, renewed) = list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 30_000))
            .await
            .unwrap();

        let requests = http.requests();
        assert_eq!(
            requests[0].url,
            "https://apigw.tuyaus.com/v1.0/m/token/renovacao-velha"
        );
        assert_eq!(header(&requests[1], "X-token"), "acesso-novo");
        assert_eq!(renewed.access_token, "acesso-novo");
        assert_eq!(renewed.refresh_token, "renovacao-nova");
        assert_eq!(renewed.expires_at_ms, NOW + 5 + 7_200_000);
    }

    #[tokio::test]
    async fn a_failed_renewal_means_the_session_expired() {
        let mut http = FakeHttp::default();
        http.respond(
            200,
            r#"{"success":false,"code":"1010","msg":"token invalid"}"#,
        );

        let result = list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW - 1)).await;

        assert_eq!(result, Err(CloudError::SessionExpired));
    }

    #[tokio::test]
    async fn api_errors_are_reported() {
        let mut http = FakeHttp::default();
        http.respond(
            200,
            r#"{"success":false,"code":"1010","msg":"token invalid"}"#,
        );
        assert_eq!(
            list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000)).await,
            Err(CloudError::SessionExpired)
        );

        http.respond(
            200,
            r#"{"success":false,"code":"2001","msg":"permission deny"}"#,
        );
        assert_eq!(
            list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000)).await,
            Err(CloudError::Rejected("permission deny".to_owned()))
        );

        http.respond(500, "erro interno");
        assert!(matches!(
            list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000)).await,
            Err(CloudError::Protocol(_))
        ));

        http.fail("sem internet");
        assert_eq!(
            list_devices(&mut http, &mut FixedEnv::new(NOW), session(NOW + 3_600_000)).await,
            Err(CloudError::Network("sem internet".to_owned()))
        );
    }

    #[test]
    fn percent_encoding_keeps_only_unreserved_characters() {
        assert_eq!(percent_encode("aZ09-_.~+/="), "aZ09-_.~%2B%2F%3D");
    }
}
