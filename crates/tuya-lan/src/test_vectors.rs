//! Acesso aos vetores gerados com o tinytuya (`tests/fixtures/vectors.json`, ver `tools/tuya-vectors`).

use serde_json::Value;

const VECTORS: &str = include_str!("../tests/fixtures/vectors.json");

pub(crate) fn vectors() -> Value {
    serde_json::from_str(VECTORS).expect("vectors.json inválido")
}

/// Um vetor de quadro: chave, seqno, comando, IV, texto puro e o quadro completo.
#[derive(Debug, Clone)]
pub(crate) struct FrameVector {
    pub(crate) name: String,
    pub(crate) key: [u8; 16],
    pub(crate) seqno: u32,
    pub(crate) command: u32,
    pub(crate) iv: [u8; 12],
    pub(crate) plaintext: Vec<u8>,
    pub(crate) frame: Vec<u8>,
}

/// Todos os quadros: os da negociação e os cifrados com a chave de sessão.
pub(crate) fn frame_vectors() -> Vec<FrameVector> {
    let all = vectors();
    let session = all["session"]["frames"].as_array().unwrap().iter();
    let after = all["frames"].as_array().unwrap().iter();
    session.chain(after).map(parse_frame).collect()
}

pub(crate) fn frame_vector(name: &str) -> FrameVector {
    frame_vectors()
        .into_iter()
        .find(|vector| vector.name == name)
        .unwrap_or_else(|| panic!("vetor {name} não existe"))
}

fn parse_frame(value: &Value) -> FrameVector {
    FrameVector {
        name: value["name"].as_str().unwrap().to_owned(),
        key: hex(value["key"].as_str().unwrap()).try_into().unwrap(),
        seqno: value["seqno"].as_u64().unwrap() as u32,
        command: value["cmd"].as_u64().unwrap() as u32,
        iv: hex(value["iv"].as_str().unwrap()).try_into().unwrap(),
        plaintext: hex(value["plaintext"].as_str().unwrap()),
        frame: hex(value["frame"].as_str().unwrap()),
    }
}

/// Decodifica texto hexadecimal (os vetores guardam bytes assim).
pub(crate) fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex inválido"))
        .collect()
}
