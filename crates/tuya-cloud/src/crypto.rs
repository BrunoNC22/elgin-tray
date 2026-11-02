//! Assinatura e criptografia das requisições da API (do `tuya_sharing/customerapi.py`).
//!
//! Para cada requisição:
//! - `hash_key` = md5 em hex de `request_id + refresh_token`;
//! - `secret` = os 16 primeiros caracteres do HMAC-SHA256 em hex de `hash_key`, com chave `request_id`;
//! - parâmetros e corpo vão como `encdata` = base64(nonce) + base64(AES-128-GCM(secret, nonce, json)),
//!   com um nonce de 12 caracteres;
//! - `X-sign` = HMAC-SHA256 em hex, com chave `hash_key`, de `"X-appKey=..||X-requestId=..||X-time=..||X-token=.."`
//!   (cabeçalhos vazios ficam de fora) seguido do `encdata` dos parâmetros e do corpo;
//! - o `result` da resposta vem como base64(nonce (12) + ciphertext + tag), cifrado com o mesmo `secret`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Chaves de uma requisição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestKeys {
    pub(crate) hash_key: String,
    pub(crate) secret: String,
}

impl RequestKeys {
    pub(crate) fn derive(request_id: &str, refresh_token: &str) -> Self {
        let hash_key = hex(&Md5::digest(format!("{request_id}{refresh_token}")));
        let mut secret = hex(&hmac(request_id.as_bytes(), hash_key.as_bytes()));
        secret.truncate(16);
        Self { hash_key, secret }
    }

    /// Cifra `json` como `encdata`. `nonce` precisa ter 12 bytes.
    pub(crate) fn encrypt(&self, json: &str, nonce: &str) -> String {
        let ciphertext = self
            .cipher()
            .encrypt(Nonce::from_slice(nonce.as_bytes()), json.as_bytes())
            .expect("o GCM só falha com mensagens de gigabytes");
        // Dois base64 concatenados (é assim que o SDK faz).
        format!("{}{}", STANDARD.encode(nonce), STANDARD.encode(ciphertext))
    }

    /// Decifra o `result` de uma resposta. `None` se não for base64 ou a autenticação falhar.
    pub(crate) fn decrypt(&self, result: &str) -> Option<String> {
        let bytes = STANDARD.decode(result).ok()?;
        if bytes.len() < 12 {
            return None;
        }
        let (nonce, ciphertext) = bytes.split_at(12);
        let plaintext = self
            .cipher()
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .ok()?;
        String::from_utf8(plaintext).ok()
    }

    /// Assinatura `X-sign` dos cabeçalhos (em ordem; vazios são ignorados) e dos `encdata`.
    pub(crate) fn sign(
        &self,
        headers: &[(&str, &str)],
        query_encdata: &str,
        body_encdata: &str,
    ) -> String {
        let signed_headers: Vec<String> = headers
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        let message = format!("{}{query_encdata}{body_encdata}", signed_headers.join("||"));
        hex(&hmac(self.hash_key.as_bytes(), message.as_bytes()))
    }

    /// Cifra como o servidor manda o `result`: base64(nonce + ciphertext + tag). Só para testes.
    #[cfg(test)]
    pub(crate) fn encrypt_result(&self, json: &str, nonce: &str) -> String {
        let ciphertext = self
            .cipher()
            .encrypt(Nonce::from_slice(nonce.as_bytes()), json.as_bytes())
            .unwrap();
        STANDARD.encode([nonce.as_bytes(), &ciphertext].concat())
    }

    fn cipher(&self) -> Aes128Gcm {
        // O secret (16 caracteres hex) é usado como os 16 bytes da chave AES.
        Aes128Gcm::new_from_slice(self.secret.as_bytes()).expect("o secret tem 16 bytes")
    }
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC aceita chave de qualquer tamanho");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vectors() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/cloud_vectors.json")).unwrap()
    }

    fn text(field: &str) -> String {
        vectors()[field].as_str().unwrap().to_owned()
    }

    fn keys() -> RequestKeys {
        RequestKeys::derive(&text("request_id"), &text("refresh_token"))
    }

    fn headers() -> Vec<(String, String)> {
        vec![
            ("X-appKey".to_owned(), text("client_id")),
            ("X-requestId".to_owned(), text("request_id")),
            ("X-sid".to_owned(), String::new()),
            ("X-time".to_owned(), text("time_ms")),
            ("X-token".to_owned(), text("access_token")),
        ]
    }

    #[test]
    fn derives_the_same_keys_as_the_sdk() {
        assert_eq!(
            keys(),
            RequestKeys {
                hash_key: text("hash_key"),
                secret: text("secret")
            }
        );
    }

    #[test]
    fn encrypts_parameters_like_the_sdk() {
        assert_eq!(
            keys().encrypt(&text("params_json"), &text("nonce")),
            text("query_encdata")
        );
    }

    #[test]
    fn signs_like_the_sdk() {
        let headers = headers();
        let borrowed: Vec<(&str, &str)> = headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();

        assert_eq!(
            keys().sign(&borrowed, &text("query_encdata"), ""),
            text("sign_with_query")
        );
        assert_eq!(keys().sign(&borrowed, "", ""), text("sign_without_query"));
    }

    #[test]
    fn decrypts_a_server_response() {
        assert_eq!(
            keys().decrypt(&text("response_result")),
            Some(text("response_json"))
        );
    }

    #[test]
    fn a_tampered_or_garbage_response_does_not_decrypt() {
        let mut tampered = text("response_result");
        tampered.replace_range(20..21, if &tampered[20..21] == "A" { "B" } else { "A" });

        assert_eq!(keys().decrypt(&tampered), None);
        assert_eq!(keys().decrypt("não é base64"), None);
        assert_eq!(keys().decrypt("QUJD"), None);
    }
}
