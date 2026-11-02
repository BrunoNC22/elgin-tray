//! Negociação da chave de sessão do protocolo 3.5.
//!
//! 1. Cliente → lâmpada (cmd 3): `local_nonce` (16 bytes), cifrado com a local key.
//! 2. Lâmpada → cliente (cmd 4): retcode + `remote_nonce` (16) + HMAC-SHA256(local_key, local_nonce).
//! 3. Cliente → lâmpada (cmd 5): HMAC-SHA256(local_key, remote_nonce), cifrado com a local key.
//!
//! Chave de sessão = ciphertext (16 bytes, sem IV e sem tag) de AES-128-GCM(local_key,
//! IV = local_nonce[..12]) sobre `local_nonce XOR remote_nonce`. Depois da negociação, todas as
//! mensagens usam a chave de sessão.
//!
//! Esta máquina de estados não faz I/O: quem tem o socket (a conexão) envia e recebe os quadros.

use std::fmt;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce as GcmNonce};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::frame::{self, Frame, FrameError, Iv, Key, command};
use crate::random::Nonce;

type HmacSha256 = Hmac<Sha256>;

const RETCODE_LEN: usize = 4;
const HMAC_LEN: usize = 32;

/// Por que a negociação falhou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HandshakeError {
    /// A lâmpada não provou conhecer a local key (HMAC ou tag inválidos): chave errada ou antiga.
    KeyRejected,
    /// A lâmpada respondeu com outro comando.
    UnexpectedCommand(u32),
    /// Resposta curta ou malformada.
    Malformed(String),
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyRejected => write!(f, "a lâmpada recusou a chave local"),
            Self::UnexpectedCommand(command) => {
                write!(f, "resposta inesperada na negociação (comando {command})")
            }
            Self::Malformed(detail) => write!(f, "resposta inválida na negociação: {detail}"),
        }
    }
}

impl std::error::Error for HandshakeError {}

impl From<FrameError> for HandshakeError {
    /// Durante a negociação os quadros usam a local key: falhar a autenticação do quadro quer dizer
    /// que a lâmpada usa outra chave.
    fn from(error: FrameError) -> Self {
        match error {
            FrameError::Authentication => Self::KeyRejected,
            other => Self::Malformed(other.to_string()),
        }
    }
}

/// Resultado de uma negociação bem-sucedida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Negotiated {
    /// Chave para todas as mensagens seguintes.
    pub(crate) session_key: Key,
    /// Payload do cmd 5, que ainda precisa ser enviado com a local key.
    pub(crate) finish_payload: [u8; HMAC_LEN],
}

/// Uma negociação em andamento.
#[derive(Debug, Clone)]
pub(crate) struct Handshake {
    local_key: Key,
    local_nonce: Nonce,
}

impl Handshake {
    pub(crate) fn new(local_key: Key, local_nonce: Nonce) -> Self {
        Self {
            local_key,
            local_nonce,
        }
    }

    /// Quadro do passo 1 (cmd 3).
    pub(crate) fn start_frame(&self, seqno: u32, iv: &Iv) -> Vec<u8> {
        frame::encode(
            seqno,
            command::SESS_KEY_NEG_START,
            &self.local_nonce,
            &self.local_key,
            iv,
        )
    }

    /// Confere a resposta da lâmpada (passo 2) e calcula a chave de sessão.
    pub(crate) fn respond(&self, response: &Frame) -> Result<Negotiated, HandshakeError> {
        if response.command != command::SESS_KEY_NEG_RESP {
            return Err(HandshakeError::UnexpectedCommand(response.command));
        }
        let body = response
            .plaintext
            .get(RETCODE_LEN..RETCODE_LEN + 16 + HMAC_LEN)
            .ok_or_else(|| {
                HandshakeError::Malformed(format!(
                    "{} bytes, esperado ao menos {}",
                    response.plaintext.len(),
                    RETCODE_LEN + 16 + HMAC_LEN
                ))
            })?;
        let (remote_nonce, proof) = body.split_at(16);

        // A lâmpada prova que conhece a local key assinando o nosso nonce.
        hmac(&self.local_key)
            .chain_update(self.local_nonce)
            .verify_slice(proof)
            .map_err(|_| HandshakeError::KeyRejected)?;

        let remote_nonce: Nonce = remote_nonce.try_into().expect("fatia de 16 bytes");
        Ok(Negotiated {
            session_key: derive_session_key(&self.local_key, &self.local_nonce, &remote_nonce),
            finish_payload: hmac(&self.local_key)
                .chain_update(remote_nonce)
                .finalize()
                .into_bytes()
                .into(),
        })
    }

    /// Quadro do passo 3 (cmd 5), cifrado com a local key.
    pub(crate) fn finish_frame(&self, negotiated: &Negotiated, seqno: u32, iv: &Iv) -> Vec<u8> {
        frame::encode(
            seqno,
            command::SESS_KEY_NEG_FINISH,
            &negotiated.finish_payload,
            &self.local_key,
            iv,
        )
    }
}

fn hmac(key: &Key) -> HmacSha256 {
    <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC aceita chave de qualquer tamanho")
}

/// Chave de sessão a partir da local key e dos dois nonces.
pub(crate) fn derive_session_key(
    local_key: &Key,
    local_nonce: &Nonce,
    remote_nonce: &Nonce,
) -> Key {
    let mut key: Key = std::array::from_fn(|i| local_nonce[i] ^ remote_nonce[i]);
    // O resultado é só o ciphertext; a tag do GCM é descartada.
    Aes128Gcm::new(local_key.into())
        .encrypt_in_place_detached(GcmNonce::from_slice(&local_nonce[..12]), &[], &mut key)
        .expect("o GCM só falha com mensagens de gigabytes");
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_vectors::{frame_vector, hex, vectors};

    struct SessionVector {
        local_key: Key,
        local_nonce: Nonce,
        remote_nonce: Nonce,
        hmac_of_local: Vec<u8>,
        hmac_of_remote: Vec<u8>,
        session_key: Key,
    }

    fn session_vector() -> SessionVector {
        let all = vectors();
        let s = &all["session"];
        let bytes = |field: &str| hex(s[field].as_str().unwrap());
        SessionVector {
            local_key: bytes("local_key").try_into().unwrap(),
            local_nonce: bytes("local_nonce").try_into().unwrap(),
            remote_nonce: bytes("remote_nonce").try_into().unwrap(),
            hmac_of_local: bytes("hmac_of_local_nonce"),
            hmac_of_remote: bytes("hmac_of_remote_nonce"),
            session_key: bytes("session_key").try_into().unwrap(),
        }
    }

    fn handshake() -> Handshake {
        let v = session_vector();
        Handshake::new(v.local_key, v.local_nonce)
    }

    /// A resposta da lâmpada (cmd 4) do vetor, já decifrada.
    fn response() -> Frame {
        let v = frame_vector("neg_resp");
        frame::decode(&v.frame, &v.key).unwrap()
    }

    #[test]
    fn derives_the_same_session_key_as_tinytuya() {
        let v = session_vector();

        assert_eq!(
            derive_session_key(&v.local_key, &v.local_nonce, &v.remote_nonce),
            v.session_key
        );
    }

    #[test]
    fn start_frame_matches_the_vector() {
        let v = frame_vector("neg_start");

        assert_eq!(handshake().start_frame(v.seqno, &v.iv), v.frame);
    }

    #[test]
    fn a_valid_response_gives_the_session_key_and_the_finish_payload() {
        let v = session_vector();

        let negotiated = handshake().respond(&response()).unwrap();

        assert_eq!(negotiated.session_key, v.session_key);
        assert_eq!(negotiated.finish_payload.to_vec(), v.hmac_of_remote);
    }

    #[test]
    fn finish_frame_matches_the_vector() {
        let v = frame_vector("neg_finish");
        let negotiated = handshake().respond(&response()).unwrap();

        assert_eq!(
            handshake().finish_frame(&negotiated, v.seqno, &v.iv),
            v.frame
        );
    }

    #[test]
    fn a_wrong_hmac_means_the_key_was_rejected() {
        let mut tampered = response();
        let last = tampered.plaintext.len() - 1;
        tampered.plaintext[last] ^= 0x01;

        assert_eq!(
            handshake().respond(&tampered),
            Err(HandshakeError::KeyRejected)
        );
    }

    #[test]
    fn a_response_for_another_local_nonce_is_rejected() {
        let v = session_vector();
        let other = Handshake::new(v.local_key, [0x42; 16]);

        assert_eq!(other.respond(&response()), Err(HandshakeError::KeyRejected));
    }

    #[test]
    fn a_response_with_another_command_is_out_of_order() {
        let mut wrong = response();
        wrong.command = command::STATUS;

        assert_eq!(
            handshake().respond(&wrong),
            Err(HandshakeError::UnexpectedCommand(command::STATUS))
        );
    }

    #[test]
    fn a_short_response_is_malformed() {
        let mut short = response();
        short.plaintext.truncate(RETCODE_LEN + 16 + 10);

        assert!(matches!(
            handshake().respond(&short),
            Err(HandshakeError::Malformed(_))
        ));
    }

    #[test]
    fn a_frame_that_fails_authentication_means_the_key_was_rejected() {
        assert_eq!(
            HandshakeError::from(FrameError::Authentication),
            HandshakeError::KeyRejected
        );
        assert!(matches!(
            HandshakeError::from(FrameError::BadSuffix),
            HandshakeError::Malformed(_)
        ));
    }

    #[test]
    fn the_hmac_vector_is_consistent() {
        // Sanidade: a resposta do vetor traz o HMAC do local nonce depois do remote nonce.
        let v = session_vector();
        let plaintext = response().plaintext;

        assert_eq!(
            &plaintext[RETCODE_LEN + 16..RETCODE_LEN + 16 + HMAC_LEN],
            v.hmac_of_local.as_slice()
        );
    }
}
