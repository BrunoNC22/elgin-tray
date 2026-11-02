//! Quadros do protocolo 3.5 (formato "6699"), cifrados com AES-128-GCM.
//!
//! ```text
//! prefixo 00006699 | reservado u16 | seqno u32 | cmd u32 | length u32 |
//! IV (12) | ciphertext | tag GCM (16) | sufixo 00009966
//! ```
//!
//! Tudo big-endian. `length` = IV + ciphertext + tag. Os dados autenticados (AAD) do GCM são os 14
//! bytes do header depois do prefixo. O texto puro é devolvido como veio: mensagens da lâmpada
//! começam com um retcode e algumas trazem o cabeçalho de versão; isso é tratado por cima desta camada.

use std::fmt;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};

pub(crate) const PREFIX: [u8; 4] = [0x00, 0x00, 0x66, 0x99];
pub(crate) const SUFFIX: [u8; 4] = [0x00, 0x00, 0x99, 0x66];

const HEADER_LEN: usize = 18;
const IV_LEN: usize = 12;
const TAG_LEN: usize = 16;
const SUFFIX_LEN: usize = 4;
/// Teto para o campo `length`: protege contra quadros corrompidos que pediriam memória demais.
const MAX_LENGTH: usize = 4096;

/// Números de comando do protocolo usados pelo app.
pub(crate) mod command {
    pub(crate) const SESS_KEY_NEG_START: u32 = 3;
    pub(crate) const SESS_KEY_NEG_RESP: u32 = 4;
    pub(crate) const SESS_KEY_NEG_FINISH: u32 = 5;
    pub(crate) const STATUS: u32 = 8;
    pub(crate) const HEART_BEAT: u32 = 9;
    pub(crate) const CONTROL_NEW: u32 = 13;
    pub(crate) const DP_QUERY_NEW: u32 = 16;
}

/// Chave AES-128 (local key ou chave de sessão).
pub(crate) type Key = [u8; 16];
/// Vetor de inicialização do GCM.
pub(crate) type Iv = [u8; 12];

/// Um quadro já decifrado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) seqno: u32,
    pub(crate) command: u32,
    pub(crate) plaintext: Vec<u8>,
}

/// Problema ao ler um quadro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameError {
    /// Os bytes não começam com o prefixo 6699.
    BadPrefix,
    /// O quadro não termina com o sufixo 9966.
    BadSuffix,
    /// O campo `length` é pequeno demais para IV + tag, ou grande demais.
    BadLength(usize),
    /// A tag do GCM não confere: chave errada ou quadro adulterado.
    Authentication,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadPrefix => write!(f, "quadro sem o prefixo 6699"),
            Self::BadSuffix => write!(f, "quadro sem o sufixo 9966"),
            Self::BadLength(length) => write!(f, "tamanho de quadro inválido: {length}"),
            Self::Authentication => write!(f, "falha na autenticação do quadro (chave errada?)"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Monta e cifra um quadro.
pub(crate) fn encode(seqno: u32, command: u32, plaintext: &[u8], key: &Key, iv: &Iv) -> Vec<u8> {
    let length = IV_LEN + plaintext.len() + TAG_LEN;
    let mut frame = Vec::with_capacity(HEADER_LEN + length + SUFFIX_LEN);
    frame.extend_from_slice(&PREFIX);
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&seqno.to_be_bytes());
    frame.extend_from_slice(&command.to_be_bytes());
    frame.extend_from_slice(&(length as u32).to_be_bytes());

    let mut ciphertext = plaintext.to_vec();
    let tag = Aes128Gcm::new(key.into())
        .encrypt_in_place_detached(
            Nonce::from_slice(iv),
            &frame[PREFIX.len()..],
            &mut ciphertext,
        )
        .expect("o GCM só falha com mensagens de gigabytes");

    frame.extend_from_slice(iv);
    frame.extend_from_slice(&ciphertext);
    frame.extend_from_slice(&tag);
    frame.extend_from_slice(&SUFFIX);
    frame
}

/// Decifra um quadro completo (exatamente um, sem bytes sobrando).
pub(crate) fn decode(bytes: &[u8], key: &Key) -> Result<Frame, FrameError> {
    if bytes.len() < HEADER_LEN {
        return Err(FrameError::BadLength(bytes.len()));
    }
    if bytes[..PREFIX.len()] != PREFIX {
        return Err(FrameError::BadPrefix);
    }
    let length = declared_length(bytes)?;
    let total = HEADER_LEN + length + SUFFIX_LEN;
    if bytes.len() != total {
        return Err(FrameError::BadLength(length));
    }
    if bytes[total - SUFFIX_LEN..] != SUFFIX {
        return Err(FrameError::BadSuffix);
    }

    let iv = &bytes[HEADER_LEN..HEADER_LEN + IV_LEN];
    let tag_start = total - SUFFIX_LEN - TAG_LEN;
    let mut plaintext = bytes[HEADER_LEN + IV_LEN..tag_start].to_vec();
    Aes128Gcm::new(key.into())
        .decrypt_in_place_detached(
            Nonce::from_slice(iv),
            &bytes[PREFIX.len()..HEADER_LEN],
            &mut plaintext,
            Tag::from_slice(&bytes[tag_start..total - SUFFIX_LEN]),
        )
        .map_err(|_| FrameError::Authentication)?;

    Ok(Frame {
        seqno: read_u32(bytes, 6),
        command: read_u32(bytes, 10),
        plaintext,
    })
}

/// O campo `length` do header, validado (precisa caber IV + tag e não passar do teto).
fn declared_length(header: &[u8]) -> Result<usize, FrameError> {
    let length = read_u32(header, 14) as usize;
    if (IV_LEN + TAG_LEN..=MAX_LENGTH).contains(&length) {
        Ok(length)
    } else {
        Err(FrameError::BadLength(length))
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut word = [0; 4];
    word.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_be_bytes(word)
}

/// Junta os pedaços que chegam do TCP e devolve quadros completos.
///
/// Bytes soltos antes de um prefixo são descartados (como faz o tinytuya), para se recuperar de
/// lixo no stream.
#[derive(Debug, Default)]
pub(crate) struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// O próximo quadro completo, se já chegou inteiro. Um quadro inválido é descartado e o erro
    /// devolvido; chamar de novo continua com o que vem depois.
    pub(crate) fn next_frame(&mut self, key: &Key) -> Result<Option<Frame>, FrameError> {
        if !self.skip_to_prefix() || self.buffer.len() < HEADER_LEN {
            return Ok(None);
        }
        let length = match declared_length(&self.buffer) {
            Ok(length) => length,
            Err(error) => {
                // Descarta só o prefixo para procurar o próximo quadro depois dele.
                self.buffer.drain(..PREFIX.len());
                return Err(error);
            }
        };
        let total = HEADER_LEN + length + SUFFIX_LEN;
        if self.buffer.len() < total {
            return Ok(None);
        }
        let bytes: Vec<u8> = self.buffer.drain(..total).collect();
        decode(&bytes, key).map(Some)
    }

    /// Descarta bytes até o próximo prefixo. Devolve `false` se não há prefixo no buffer (guarda
    /// só o final, que pode ser o começo de um prefixo ainda incompleto).
    fn skip_to_prefix(&mut self) -> bool {
        match self
            .buffer
            .windows(PREFIX.len())
            .position(|window| window == PREFIX)
        {
            Some(start) => {
                self.buffer.drain(..start);
                true
            }
            None => {
                let keep = self.buffer.len().min(PREFIX.len() - 1);
                self.buffer.drain(..self.buffer.len() - keep);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_vectors::{frame_vector, frame_vectors};

    #[test]
    fn encodes_every_vector_byte_for_byte() {
        for v in frame_vectors() {
            let frame = encode(v.seqno, v.command, &v.plaintext, &v.key, &v.iv);
            assert_eq!(frame, v.frame, "vetor {}", v.name);
        }
    }

    #[test]
    fn decodes_every_vector() {
        for v in frame_vectors() {
            let frame = decode(&v.frame, &v.key).unwrap_or_else(|e| panic!("{}: {e}", v.name));
            assert_eq!(
                frame,
                Frame {
                    seqno: v.seqno,
                    command: v.command,
                    plaintext: v.plaintext.clone()
                },
                "vetor {}",
                v.name
            );
        }
    }

    #[test]
    fn a_wrong_key_fails_authentication() {
        let v = frame_vector("control_new");

        assert_eq!(
            decode(&v.frame, &[0u8; 16]),
            Err(FrameError::Authentication)
        );
    }

    #[test]
    fn a_tampered_byte_fails_authentication() {
        let v = frame_vector("status_push");
        let mut tampered = v.frame.clone();
        tampered[HEADER_LEN + IV_LEN + 2] ^= 0x01;

        assert_eq!(decode(&tampered, &v.key), Err(FrameError::Authentication));
    }

    #[test]
    fn a_tampered_header_fails_authentication() {
        let v = frame_vector("status_push");
        let mut tampered = v.frame.clone();
        tampered[9] ^= 0x01; // seqno faz parte dos dados autenticados

        assert_eq!(decode(&tampered, &v.key), Err(FrameError::Authentication));
    }

    #[test]
    fn rejects_bad_prefix_suffix_and_length() {
        let v = frame_vector("heart_beat");

        let mut bad_prefix = v.frame.clone();
        bad_prefix[2] = 0x55;
        assert_eq!(decode(&bad_prefix, &v.key), Err(FrameError::BadPrefix));

        let mut bad_suffix = v.frame.clone();
        let last = bad_suffix.len() - 1;
        bad_suffix[last] = 0x00;
        assert_eq!(decode(&bad_suffix, &v.key), Err(FrameError::BadSuffix));

        let mut too_short = v.frame[..HEADER_LEN].to_vec();
        too_short[14..18].copy_from_slice(&10u32.to_be_bytes());
        too_short.extend_from_slice(&[0; 10]);
        too_short.extend_from_slice(&SUFFIX);
        assert_eq!(decode(&too_short, &v.key), Err(FrameError::BadLength(10)));

        assert_eq!(
            decode(&v.frame[..v.frame.len() - 1], &v.key),
            Err(FrameError::BadLength(
                v.frame.len() - HEADER_LEN - SUFFIX_LEN
            ))
        );
    }

    #[test]
    fn decoder_waits_for_the_whole_frame() {
        let v = frame_vector("dp_query_new_resp");
        let mut decoder = FrameDecoder::new();

        decoder.push(&v.frame[..10]);
        assert_eq!(decoder.next_frame(&v.key), Ok(None));
        decoder.push(&v.frame[10..v.frame.len() - 1]);
        assert_eq!(decoder.next_frame(&v.key), Ok(None));
        decoder.push(&v.frame[v.frame.len() - 1..]);

        assert_eq!(
            decoder.next_frame(&v.key).unwrap().unwrap().plaintext,
            v.plaintext
        );
        assert_eq!(decoder.next_frame(&v.key), Ok(None));
    }

    #[test]
    fn decoder_splits_two_frames_in_the_same_read() {
        let first = frame_vector("heart_beat_resp");
        let second = frame_vector("status_push");
        let mut decoder = FrameDecoder::new();

        decoder.push(&[first.frame.clone(), second.frame.clone()].concat());

        assert_eq!(
            decoder.next_frame(&first.key).unwrap().unwrap().command,
            first.command
        );
        assert_eq!(
            decoder.next_frame(&second.key).unwrap().unwrap().command,
            second.command
        );
        assert_eq!(decoder.next_frame(&second.key), Ok(None));
    }

    #[test]
    fn decoder_skips_garbage_before_a_frame() {
        let v = frame_vector("control_new_ack");
        let mut decoder = FrameDecoder::new();

        decoder.push(&[0xde, 0xad, 0x00, 0x00, 0x66]);
        decoder.push(&v.frame);

        assert_eq!(decoder.next_frame(&v.key).unwrap().unwrap().seqno, v.seqno);
    }

    #[test]
    fn decoder_drops_an_invalid_frame_and_continues() {
        let bad = frame_vector("heart_beat_resp");
        let good = frame_vector("status_push");
        let mut tampered = bad.frame.clone();
        tampered[HEADER_LEN + 1] ^= 0xff;
        let mut decoder = FrameDecoder::new();

        decoder.push(&[tampered, good.frame.clone()].concat());

        assert_eq!(
            decoder.next_frame(&bad.key),
            Err(FrameError::Authentication)
        );
        assert_eq!(
            decoder.next_frame(&good.key).unwrap().unwrap().seqno,
            good.seqno
        );
    }

    #[test]
    fn decoder_drops_a_frame_with_an_absurd_length() {
        let good = frame_vector("status_push");
        let mut absurd = PREFIX.to_vec();
        absurd.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 0, 0, 8]);
        absurd.extend_from_slice(&(1_000_000u32).to_be_bytes());
        let mut decoder = FrameDecoder::new();

        decoder.push(&[absurd, good.frame.clone()].concat());

        assert_eq!(
            decoder.next_frame(&good.key),
            Err(FrameError::BadLength(1_000_000))
        );
        assert_eq!(
            decoder.next_frame(&good.key).unwrap().unwrap().seqno,
            good.seqno
        );
    }
}
