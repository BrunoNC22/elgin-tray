//! Adaptador: protocolo local Tuya 3.5 (TCP 6668) e descoberta UDP.
//!
//! Referência do protocolo: tinytuya (ver `tools/tuya-vectors/README.md`).

// As partes do protocolo são construídas em issues separadas e só são usadas pelo adaptador
// completo (#12). Até lá, parte delas só é usada nos testes.
#![allow(dead_code)]

mod frame;
mod random;
mod session;

#[cfg(test)]
mod test_vectors;
