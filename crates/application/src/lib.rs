//! Aplicação: casos de uso e portas (traits). Dublês das portas na feature `testing`.

/// Dublês de teste das portas (stubs, spies, fakes), escritos à mão.
#[cfg(any(test, feature = "testing"))]
pub mod testing {}
