//! Uma mensagem JSON por linha. O decoder junta pedaços e separa várias mensagens numa leitura.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Linha maior que isto é descartada (protege contra um par que manda lixo sem fim).
pub const MAX_LINE: usize = 64 * 1024;

/// Problema ao ler uma linha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// A linha não é uma mensagem válida (ela é descartada; as próximas continuam).
    Invalid(String),
    /// A linha passou do tamanho máximo e foi descartada.
    TooLong,
}

/// Mensagem em JSON com `\n` no fim.
pub fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(message).expect("as mensagens sempre viram JSON");
    bytes.push(b'\n');
    bytes
}

/// Junta bytes e devolve mensagens inteiras.
#[derive(Debug, Default)]
pub struct LineDecoder {
    buffer: Vec<u8>,
    /// Descartando o resto de uma linha longa demais, até o próximo `\n`.
    skipping: bool,
}

impl LineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// A próxima mensagem completa; `None` se ainda não chegou uma linha inteira.
    pub fn next<T: DeserializeOwned>(&mut self) -> Option<Result<T, CodecError>> {
        loop {
            let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') else {
                if self.buffer.len() > MAX_LINE {
                    // Joga fora o que chegou e o resto da linha, até o próximo `\n`.
                    self.buffer.clear();
                    let was_skipping = std::mem::replace(&mut self.skipping, true);
                    return (!was_skipping).then_some(Err(CodecError::TooLong));
                }
                if self.skipping {
                    self.buffer.clear();
                }
                return None;
            };
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            if std::mem::take(&mut self.skipping) {
                continue;
            }
            let text = line.trim_ascii();
            if text.is_empty() {
                continue;
            }
            return Some(
                serde_json::from_slice(text)
                    .map_err(|error| CodecError::Invalid(error.to_string())),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::PanelMessage;

    fn hello() -> PanelMessage {
        PanelMessage::Hello {
            protocol_version: 1,
        }
    }

    #[test]
    fn encodes_one_json_line() {
        assert_eq!(encode(&PanelMessage::Close), b"{\"type\":\"close\"}\n");
    }

    #[test]
    fn decodes_a_message_split_in_pieces() {
        let bytes = encode(&hello());
        let mut decoder = LineDecoder::new();

        decoder.push(&bytes[..5]);
        assert_eq!(decoder.next::<PanelMessage>(), None);
        decoder.push(&bytes[5..]);

        assert_eq!(decoder.next::<PanelMessage>(), Some(Ok(hello())));
        assert_eq!(decoder.next::<PanelMessage>(), None);
    }

    #[test]
    fn decodes_several_messages_from_one_read() {
        let mut decoder = LineDecoder::new();
        decoder.push(&[encode(&hello()), encode(&PanelMessage::Close)].concat());

        assert_eq!(decoder.next::<PanelMessage>(), Some(Ok(hello())));
        assert_eq!(
            decoder.next::<PanelMessage>(),
            Some(Ok(PanelMessage::Close))
        );
        assert_eq!(decoder.next::<PanelMessage>(), None);
    }

    #[test]
    fn an_invalid_line_is_reported_and_the_next_one_still_works() {
        let mut decoder = LineDecoder::new();
        decoder.push(b"isto nao e json\n");
        decoder.push(&encode(&PanelMessage::Close));

        assert!(matches!(
            decoder.next::<PanelMessage>(),
            Some(Err(CodecError::Invalid(_)))
        ));
        assert_eq!(
            decoder.next::<PanelMessage>(),
            Some(Ok(PanelMessage::Close))
        );
    }

    #[test]
    fn empty_lines_are_ignored() {
        let mut decoder = LineDecoder::new();
        decoder.push(b"\n\r\n");
        decoder.push(&encode(&PanelMessage::Close));

        assert_eq!(
            decoder.next::<PanelMessage>(),
            Some(Ok(PanelMessage::Close))
        );
    }

    #[test]
    fn a_line_that_is_too_long_is_dropped() {
        let mut decoder = LineDecoder::new();
        decoder.push(&vec![b'x'; MAX_LINE + 1]);
        assert_eq!(
            decoder.next::<PanelMessage>(),
            Some(Err(CodecError::TooLong))
        );

        decoder.push(b"resto da linha longa\n");
        decoder.push(&encode(&PanelMessage::Close));
        assert_eq!(
            decoder.next::<PanelMessage>(),
            Some(Ok(PanelMessage::Close))
        );
    }
}
