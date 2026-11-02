//! Valores aleatórios do protocolo (IVs do GCM e nonces da negociação), injetáveis para teste.

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;

use crate::frame::Iv;

/// Nonce de 16 bytes da negociação de sessão.
pub(crate) type Nonce = [u8; 16];

/// Fonte de valores aleatórios. Cada IV precisa ser único por chave: nunca reutilizar.
pub(crate) trait Randomness {
    fn iv(&mut self) -> Iv;
    fn nonce(&mut self) -> Nonce;
}

/// Aleatoriedade do sistema operacional.
#[derive(Debug, Default)]
pub(crate) struct OsRandomness;

impl Randomness for OsRandomness {
    fn iv(&mut self) -> Iv {
        let mut iv = [0; 12];
        OsRng.fill_bytes(&mut iv);
        iv
    }

    fn nonce(&mut self) -> Nonce {
        let mut nonce = [0; 16];
        OsRng.fill_bytes(&mut nonce);
        nonce
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_randomness_does_not_repeat() {
        let mut random = OsRandomness;

        assert_ne!(random.iv(), random.iv());
        assert_ne!(random.nonce(), random.nonce());
    }
}
