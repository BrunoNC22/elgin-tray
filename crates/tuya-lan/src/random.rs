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

/// Valores roteirizados, para testes com os vetores do tinytuya.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct ScriptedRandomness {
    pub(crate) ivs: std::collections::VecDeque<Iv>,
    pub(crate) nonces: std::collections::VecDeque<Nonce>,
}

#[cfg(test)]
impl Randomness for ScriptedRandomness {
    fn iv(&mut self) -> Iv {
        self.ivs.pop_front().expect("nenhum IV roteirizado")
    }

    fn nonce(&mut self) -> Nonce {
        self.nonces.pop_front().expect("nenhum nonce roteirizado")
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
