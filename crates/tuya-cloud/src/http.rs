//! HTTP atrás de um trait: o adaptador real usa `ureq`; os testes, respostas gravadas.

use std::time::Duration;

/// Método HTTP usado pela API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Corpo JSON, se houver.
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Faz requisições HTTP. O erro é uma descrição de falha de rede (sem resposta do servidor).
pub trait HttpClient {
    async fn send(&mut self, request: HttpRequest) -> Result<HttpResponse, String>;
}

/// Cliente real (`ureq` com rustls). As requisições rodam numa thread de bloqueio do tokio.
#[derive(Debug, Clone)]
pub struct UreqClient {
    agent: ureq::Agent,
}

impl Default for UreqClient {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            // Respostas 4xx/5xx também têm corpo JSON com a mensagem do servidor.
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent }
    }
}

impl HttpClient for UreqClient {
    async fn send(&mut self, request: HttpRequest) -> Result<HttpResponse, String> {
        let agent = self.agent.clone();
        tokio::task::spawn_blocking(move || send_blocking(&agent, request))
            .await
            .map_err(|error| error.to_string())?
    }
}

fn send_blocking(agent: &ureq::Agent, request: HttpRequest) -> Result<HttpResponse, String> {
    let response = match request.method {
        Method::Get => {
            let mut builder = agent.get(&request.url);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            builder.call()
        }
        Method::Post => {
            let mut builder = agent
                .post(&request.url)
                .header("Content-Type", "application/json");
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            match request.body {
                Some(body) => builder.send(body),
                None => builder.send_empty(),
            }
        }
    };
    let mut response = response.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    Ok(HttpResponse { status, body })
}

/// HTTP falso: devolve respostas roteirizadas em ordem e grava as requisições.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeHttp {
    inner: std::rc::Rc<std::cell::RefCell<FakeInner>>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct FakeInner {
    responses: std::collections::VecDeque<Result<HttpResponse, String>>,
    requests: Vec<HttpRequest>,
}

#[cfg(test)]
impl FakeHttp {
    pub(crate) fn respond(&self, status: u16, body: &str) {
        self.inner
            .borrow_mut()
            .responses
            .push_back(Ok(HttpResponse {
                status,
                body: body.to_owned(),
            }));
    }

    pub(crate) fn fail(&self, error: &str) {
        self.inner
            .borrow_mut()
            .responses
            .push_back(Err(error.to_owned()));
    }

    pub(crate) fn requests(&self) -> Vec<HttpRequest> {
        self.inner.borrow().requests.clone()
    }
}

#[cfg(test)]
impl HttpClient for FakeHttp {
    async fn send(&mut self, request: HttpRequest) -> Result<HttpResponse, String> {
        let mut inner = self.inner.borrow_mut();
        inner.requests.push(request);
        inner
            .responses
            .pop_front()
            .expect("nenhuma resposta roteirizada")
    }
}
