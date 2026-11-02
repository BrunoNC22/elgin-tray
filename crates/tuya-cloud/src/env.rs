//! O que muda a cada chamada (ids aleatórios e o relógio), injetável para os testes.

use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;

/// Valores aleatórios e horário usados nas requisições.
pub trait CloudEnv {
    /// Id da requisição (um UUID v4 em texto, como o SDK).
    fn request_id(&mut self) -> String;
    /// Nonce de 12 caracteres para cifrar parâmetros.
    fn nonce(&mut self) -> String;
    /// Agora, em milissegundos desde 1970.
    fn now_ms(&self) -> u64;
}

/// Aleatoriedade do sistema operacional e relógio do sistema.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

/// Mesmo alfabeto do SDK (sem caracteres ambíguos).
const NONCE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZabcdefhijkmnprstwxyz2345678";

impl CloudEnv for SystemEnv {
    fn request_id(&mut self) -> String {
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        bytes[6] = (bytes[6] & 0x0f) | 0x40; // versão 4
        bytes[8] = (bytes[8] & 0x3f) | 0x80; // variante RFC 4122
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        )
    }

    fn nonce(&mut self) -> String {
        let mut bytes = [0u8; 12];
        OsRng.fill_bytes(&mut bytes);
        bytes
            .iter()
            .map(|byte| char::from(NONCE_ALPHABET[usize::from(*byte) % NONCE_ALPHABET.len()]))
            .collect()
    }

    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_look_like_uuid_v4_and_do_not_repeat() {
        let mut env = SystemEnv;
        let id = env.request_id();

        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_eq!(id.matches('-').count(), 4);
        assert_ne!(id, env.request_id());
    }

    #[test]
    fn nonces_have_12_characters_of_the_alphabet() {
        let nonce = SystemEnv.nonce();

        assert_eq!(nonce.len(), 12);
        assert!(nonce.bytes().all(|byte| NONCE_ALPHABET.contains(&byte)));
    }
}
