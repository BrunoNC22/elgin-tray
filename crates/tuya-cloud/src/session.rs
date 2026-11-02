//! Conteúdo da sessão da nuvem. Para a aplicação é um texto opaco (`CloudSession`); aqui é JSON.

use elgin_application::config::CloudSession;
use elgin_application::ports::CloudError;
use serde::{Deserialize, Serialize};

/// O que é preciso para chamar a API depois do login (o mesmo que o SDK da Tuya guarda).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionData {
    pub(crate) user_code: String,
    pub(crate) terminal_id: String,
    /// Servidor da região da conta (ex.: `https://apigw.tuyaus.com`).
    pub(crate) endpoint: String,
    pub(crate) uid: String,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    /// Quando o `access_token` expira, em milissegundos desde 1970.
    pub(crate) expires_at_ms: u64,
}

impl SessionData {
    pub(crate) fn to_cloud_session(&self) -> CloudSession {
        CloudSession::new(serde_json::to_string(self).expect("SessionData sempre vira JSON"))
    }

    // Usado pela listagem de dispositivos (#17).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_cloud_session(session: &CloudSession) -> Result<Self, CloudError> {
        // Uma sessão que não dá para ler é tratada como expirada: o caminho é fazer login de novo.
        serde_json::from_str(session.expose()).map_err(|_| CloudError::SessionExpired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionData {
        SessionData {
            user_code: "Abc1234".to_owned(),
            terminal_id: "terminal".to_owned(),
            endpoint: "https://apigw.tuyaus.com".to_owned(),
            uid: "az123".to_owned(),
            access_token: "acesso".to_owned(),
            refresh_token: "renovacao".to_owned(),
            expires_at_ms: 1_700_007_200_000,
        }
    }

    #[test]
    fn round_trips_through_the_opaque_cloud_session() {
        let cloud = session().to_cloud_session();

        assert_eq!(SessionData::from_cloud_session(&cloud), Ok(session()));
    }

    #[test]
    fn an_unreadable_session_is_treated_as_expired() {
        let broken = CloudSession::new("não é json");

        assert_eq!(
            SessionData::from_cloud_session(&broken),
            Err(CloudError::SessionExpired)
        );
    }
}
