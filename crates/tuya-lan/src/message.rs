//! Conteúdo das mensagens (o texto puro dentro dos quadros) no protocolo 3.5.

use serde_json::{Map, Value};

/// Cabeçalho de versão que vai antes do JSON em comandos como `CONTROL_NEW` e chega nos pushes.
pub(crate) const VERSION_HEADER: [u8; 15] = *b"3.5\0\0\0\0\0\0\0\0\0\0\0\0";

const RETCODE_LEN: usize = 4;

/// Consulta de estado (`DP_QUERY_NEW`): JSON vazio, sem cabeçalho de versão.
pub(crate) fn query_payload() -> Vec<u8> {
    b"{}".to_vec()
}

/// Heartbeat: `{"gwId":<id>,"devId":<id>}`, sem cabeçalho de versão.
pub(crate) fn heartbeat_payload(device_id: &str) -> Vec<u8> {
    let id = Value::from(device_id).to_string();
    format!(r#"{{"gwId":{id},"devId":{id}}}"#).into_bytes()
}

/// Comando (`CONTROL_NEW`): cabeçalho de versão + `{"protocol":5,"t":<epoch>,"data":{"dps":{...}}}`.
///
/// O JSON é montado na mesma ordem de chaves do tinytuya, sem espaços.
pub(crate) fn control_payload(dps: &Map<String, Value>, timestamp: u64) -> Vec<u8> {
    let dps = Value::Object(dps.clone()).to_string();
    let json = format!(r#"{{"protocol":5,"t":{timestamp},"data":{{"dps":{dps}}}}}"#);
    [VERSION_HEADER.as_slice(), json.as_bytes()].concat()
}

/// Mensagem vinda da lâmpada, sem o retcode e sem o cabeçalho de versão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceMessage {
    pub(crate) retcode: u32,
    pub(crate) body: Vec<u8>,
}

impl DeviceMessage {
    /// Separa retcode, cabeçalho de versão (se houver) e corpo. `None` se não há nem o retcode.
    pub(crate) fn parse(plaintext: &[u8]) -> Option<Self> {
        let (retcode, rest) = plaintext.split_first_chunk::<RETCODE_LEN>()?;
        // A versão vem sozinha nos 3 primeiros bytes ("3.5"); o cabeçalho inteiro tem 15.
        let body = match rest.strip_prefix(&VERSION_HEADER[..3]) {
            Some(_) => rest.get(VERSION_HEADER.len()..).unwrap_or_default(),
            None => rest,
        };
        Some(Self {
            retcode: u32::from_be_bytes(*retcode),
            body: body.to_vec(),
        })
    }

    /// Os DPs do corpo: em `{"dps":{...}}` (resposta de consulta) ou `{"data":{"dps":{...}}}` (push).
    pub(crate) fn dps(&self) -> Option<Map<String, Value>> {
        let json: Value = serde_json::from_slice(&self.body).ok()?;
        let dps = json.get("dps").or_else(|| json.get("data")?.get("dps"))?;
        dps.as_object().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_vectors::{frame_vector, hex, vectors};
    use serde_json::json;

    fn vector_payload(name: &str) -> Vec<u8> {
        let all = vectors();
        let frames = all["frames"].as_array().unwrap();
        let frame = frames.iter().find(|f| f["name"] == name).unwrap();
        hex(frame["payload"].as_str().unwrap())
    }

    fn device_id() -> String {
        vectors()["device_id"].as_str().unwrap().to_owned()
    }

    #[test]
    fn query_payload_matches_tinytuya() {
        assert_eq!(query_payload(), vector_payload("dp_query_new"));
    }

    #[test]
    fn heartbeat_payload_matches_tinytuya() {
        assert_eq!(
            heartbeat_payload(&device_id()),
            vector_payload("heart_beat")
        );
    }

    #[test]
    fn control_payload_matches_tinytuya() {
        let dps = json!({"20": true, "21": "colour", "24": "00f003e803e8"});

        assert_eq!(
            control_payload(dps.as_object().unwrap(), 1_700_000_000),
            vector_payload("control_new")
        );
    }

    #[test]
    fn version_header_matches_the_vectors() {
        assert_eq!(
            VERSION_HEADER.to_vec(),
            hex(vectors()["version_header"].as_str().unwrap())
        );
    }

    #[test]
    fn parses_a_query_response() {
        let message = DeviceMessage::parse(&frame_vector("dp_query_new_resp").plaintext).unwrap();

        assert_eq!(message.retcode, 0);
        assert_eq!(
            message.dps(),
            Some(json!({"20": true, "21": "white", "22": 1000, "23": 1000, "24": "000003e803e8", "26": 0})
                .as_object()
                .unwrap()
                .clone())
        );
    }

    #[test]
    fn parses_a_push_with_version_header() {
        let message = DeviceMessage::parse(&frame_vector("status_push").plaintext).unwrap();

        assert_eq!(
            message.dps(),
            Some(json!({"22": 500}).as_object().unwrap().clone())
        );
    }

    #[test]
    fn an_acknowledgement_has_only_the_retcode() {
        let message = DeviceMessage::parse(&frame_vector("control_new_ack").plaintext).unwrap();

        assert_eq!(
            message,
            DeviceMessage {
                retcode: 0,
                body: Vec::new()
            }
        );
        assert_eq!(message.dps(), None);
    }

    #[test]
    fn reads_a_non_zero_retcode() {
        let message = DeviceMessage::parse(&[0, 0, 0, 1]).unwrap();

        assert_eq!(message.retcode, 1);
    }

    #[test]
    fn a_message_shorter_than_the_retcode_is_invalid() {
        assert_eq!(DeviceMessage::parse(&[0, 0, 0]), None);
    }

    #[test]
    fn a_body_that_is_not_json_has_no_dps() {
        let message = DeviceMessage::parse(b"\0\0\0\0data unvalid").unwrap();

        assert_eq!(message.dps(), None);
    }
}
