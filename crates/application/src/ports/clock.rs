use std::time::Instant;

/// Relógio. Injetado para os testes controlarem o tempo sem esperar de verdade.
pub trait Clock {
    fn now(&self) -> Instant;
}
