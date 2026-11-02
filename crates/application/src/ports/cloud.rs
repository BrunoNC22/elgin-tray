use std::fmt;

use crate::config::CloudSession;
use crate::credentials::LocalKey;

/// Conta na nuvem do fabricante (Smart Life), usada só para configurar a lâmpada e recuperar a
/// local key. O controle do dia a dia é todo pela rede local.
pub trait CloudAccount {
    /// Começa um login por QR code com o "user code" da conta (Smart Life → Eu → engrenagem →
    /// Conta e segurança → Código de usuário).
    async fn start_qr_login(&mut self, user_code: &str) -> Result<QrLogin, CloudError>;

    /// Consulta se o QR já foi lido e confirmado no celular.
    async fn poll_login(&mut self, login: &QrLogin) -> Result<LoginPoll, CloudError>;

    /// Lista os dispositivos da conta, com a local key de cada um. Renova a sessão se precisar: a
    /// sessão devolvida (talvez nova) deve ser salva no lugar da antiga.
    async fn list_devices(&mut self, session: &CloudSession) -> Result<CloudDevices, CloudError>;
}

/// Um dispositivo da conta na nuvem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudDevice {
    pub id: String,
    pub name: String,
    pub local_key: LocalKey,
    /// Categoria como o fabricante chama (só para mostrar).
    pub category: String,
    /// Se é uma lâmpada ou fita de luz (decidido pelo adaptador).
    pub is_light: bool,
    pub product_name: String,
}

/// Resultado da listagem: os dispositivos e a sessão (renovada, se foi preciso).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudDevices {
    pub devices: Vec<CloudDevice>,
    pub session: CloudSession,
}

/// Um login por QR em andamento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrLogin {
    pub user_code: String,
    /// Identifica o login no servidor.
    pub token: String,
    /// O texto a desenhar como QR code para o app do celular ler.
    pub qr_content: String,
}

/// Resultado de uma consulta do login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginPoll {
    /// Ainda não confirmado no celular.
    Pending,
    /// Confirmado: a sessão para as próximas chamadas (guardar no config).
    Done(CloudSession),
}

/// Falha ao falar com a nuvem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudError {
    /// O servidor recusou o pedido (ex.: user code inválido). A mensagem vem do servidor.
    Rejected(String),
    /// Sem conexão com o servidor.
    Network(String),
    /// Resposta inesperada.
    Protocol(String),
    /// A sessão salva não vale mais: é preciso fazer o login de novo.
    SessionExpired,
}

impl fmt::Display for CloudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(message) => write!(f, "a nuvem recusou o pedido: {message}"),
            Self::Network(detail) => write!(f, "sem conexão com a nuvem: {detail}"),
            Self::Protocol(detail) => write!(f, "resposta inesperada da nuvem: {detail}"),
            Self::SessionExpired => write!(f, "a sessão da nuvem expirou; faça o login de novo"),
        }
    }
}

impl std::error::Error for CloudError {}
