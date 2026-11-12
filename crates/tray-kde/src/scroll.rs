//! Rolagem do mouse sobre o ícone.

/// Quanto o Plasma manda por "dente" da roda do mouse (o `angleDelta` do Qt).
const NOTCH: i32 = 120;

/// Converte a rolagem do Plasma (positiva para cima) em passos inteiros. Touchpads mandam pedaços
/// menores que um dente: o resto fica guardado para a próxima rolagem (e é descartado se a
/// direção muda).
#[derive(Debug, Default)]
pub(crate) struct ScrollSteps {
    pending: i32,
}

impl ScrollSteps {
    /// Soma `delta` e devolve quantos passos inteiros já deu (0 se ainda não completou um dente).
    pub(crate) fn add(&mut self, delta: i32) -> i32 {
        if self.pending.signum() * delta.signum() < 0 {
            self.pending = 0;
        }
        self.pending = self.pending.saturating_add(delta);
        let steps = self.pending / NOTCH;
        self.pending -= steps * NOTCH;
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mouse_notch_is_one_step() {
        let mut scroll = ScrollSteps::default();

        assert_eq!(scroll.add(120), 1);
        assert_eq!(scroll.add(-240), -2);
    }

    #[test]
    fn small_touchpad_deltas_add_up_to_a_step() {
        let mut scroll = ScrollSteps::default();

        assert_eq!(scroll.add(50), 0);
        assert_eq!(scroll.add(50), 0);
        assert_eq!(scroll.add(50), 1);
        assert_eq!(scroll.add(-30), 0);
        assert_eq!(scroll.add(-90), -1);
    }

    #[test]
    fn changing_direction_discards_the_leftover() {
        let mut scroll = ScrollSteps::default();
        scroll.add(100);

        assert_eq!(scroll.add(-120), -1);
    }
}
