//! HTTP atrás de um trait: o adaptador real usa `ureq`; os testes, respostas prontas.

use std::time::Duration;

/// Maior download aceito (o pacote do Windows tem poucos MB).
const MAX_BODY: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Faz GETs. O erro é uma descrição de falha de rede (sem resposta do servidor).
pub trait HttpGet {
    async fn get(&mut self, url: &str) -> Result<HttpResponse, String>;
}

/// Cliente real (`ureq` com rustls, segue redirecionamentos). As requisições rodam numa thread de
/// bloqueio do tokio, para o download não travar o daemon.
#[derive(Debug, Clone)]
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .user_agent(concat!("elgin-tray/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }
}

impl HttpGet for UreqHttp {
    async fn get(&mut self, url: &str) -> Result<HttpResponse, String> {
        let agent = self.agent.clone();
        let url = url.to_owned();
        tokio::task::spawn_blocking(move || get_blocking(&agent, &url))
            .await
            .map_err(|error| error.to_string())?
    }
}

fn get_blocking(agent: &ureq::Agent, url: &str) -> Result<HttpResponse, String> {
    let mut response = agent
        .get(url)
        // A API do GitHub pede este Accept; os downloads ignoram.
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .map_err(|error| error.to_string())?;
    Ok(HttpResponse { status, body })
}

#[cfg(test)]
pub(crate) mod testing {
    use std::collections::HashMap;

    use super::*;

    /// Respostas prontas por URL; URL sem resposta é falha de rede. Guarda as URLs pedidas.
    #[derive(Debug, Default)]
    pub(crate) struct FakeHttp {
        responses: HashMap<String, HttpResponse>,
        pub(crate) requested: Vec<String>,
    }

    impl FakeHttp {
        pub(crate) fn respond(&mut self, url: &str, status: u16, body: impl Into<Vec<u8>>) {
            self.responses.insert(
                url.to_owned(),
                HttpResponse {
                    status,
                    body: body.into(),
                },
            );
        }
    }

    impl HttpGet for FakeHttp {
        async fn get(&mut self, url: &str) -> Result<HttpResponse, String> {
            self.requested.push(url.to_owned());
            self.responses
                .get(url)
                .cloned()
                .ok_or_else(|| "sem conexão (roteiro)".to_owned())
        }
    }
}
