use crate::brightness::Brightness;
use crate::bulb::{BulbState, LightMode, Power};
use crate::colour::Hsv;
use crate::temperature::ColorTemperature;

/// Uma mudança pedida pelo usuário (bandeja, painel, atalho ou CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BulbCommand {
    SetPower(Power),
    TogglePower,
    /// Muda para o modo cor com esta cor e brilho.
    SetColour(Hsv),
    /// Muda para o modo branco com este brilho e temperatura.
    SetWhite {
        brightness: Brightness,
        temperature: ColorTemperature,
    },
    /// Muda a temperatura (e vai para o modo branco, se não estiver nele).
    SetTemperature(ColorTemperature),
    /// Muda o brilho do modo atual.
    SetBrightness(Brightness),
    /// Soma (ou subtrai) pontos percentuais ao brilho do modo atual, sem sair de 1–100%.
    StepBrightness(i8),
}

impl BulbState {
    /// Estado esperado depois de executar `command` (base da atualização otimista).
    ///
    /// Regras:
    /// - Comandos de cor, branco, temperatura e brilho **ligam** a lâmpada: quem mexe na cor
    ///   espera ver o resultado.
    /// - O brilho nunca chega a apagar a lâmpada: fica entre 1% e 100%. Só `SetPower`/`TogglePower`
    ///   desligam.
    /// - Em cena ou música não há um brilho único para mudar: comandos de brilho só ligam a lâmpada.
    /// - `SetTemperature` fora do modo branco vai para o branco mantendo o brilho atual
    ///   (ou 100%, se o modo não tem brilho).
    pub fn apply(&self, command: &BulbCommand) -> BulbState {
        match *command {
            BulbCommand::SetPower(power) => BulbState { power, ..*self },
            BulbCommand::TogglePower => BulbState {
                power: self.power.toggled(),
                ..*self
            },
            BulbCommand::SetColour(hsv) => turned_on(LightMode::Colour(hsv)),
            BulbCommand::SetWhite {
                brightness,
                temperature,
            } => turned_on(LightMode::White {
                brightness,
                temperature,
            }),
            BulbCommand::SetTemperature(temperature) => turned_on(LightMode::White {
                brightness: self.mode.brightness().unwrap_or(Brightness::MAX),
                temperature,
            }),
            BulbCommand::SetBrightness(brightness) => {
                turned_on(with_brightness(self.mode, brightness))
            }
            BulbCommand::StepBrightness(delta) => match self.mode.brightness() {
                Some(current) => turned_on(with_brightness(self.mode, current.stepped(delta))),
                None => turned_on(self.mode),
            },
        }
    }
}

fn turned_on(mode: LightMode) -> BulbState {
    BulbState {
        power: Power::On,
        mode,
    }
}

/// O mesmo modo com outro brilho. Cena e música não têm brilho para trocar e ficam iguais.
fn with_brightness(mode: LightMode, brightness: Brightness) -> LightMode {
    match mode {
        LightMode::White { temperature, .. } => LightMode::White {
            brightness,
            temperature,
        },
        LightMode::Colour(hsv) => LightMode::Colour(hsv.with_value(brightness)),
        LightMode::Scene | LightMode::Music => mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colour::{Hue, Saturation};

    fn brightness(permille: u16) -> Brightness {
        Brightness::try_from_permille(permille).unwrap()
    }

    fn temperature(permille: u16) -> ColorTemperature {
        ColorTemperature::try_from_permille(permille).unwrap()
    }

    fn colour(hue: u16, value: u16) -> Hsv {
        Hsv::new(Hue::new(hue), Saturation::FULL, brightness(value))
    }

    fn white(brightness_permille: u16, temperature_permille: u16) -> LightMode {
        LightMode::White {
            brightness: brightness(brightness_permille),
            temperature: temperature(temperature_permille),
        }
    }

    fn state(power: Power, mode: LightMode) -> BulbState {
        BulbState { power, mode }
    }

    #[test]
    fn set_power_changes_only_the_power() {
        let on = state(Power::On, white(500, 200));

        assert_eq!(
            on.apply(&BulbCommand::SetPower(Power::Off)),
            state(Power::Off, white(500, 200))
        );
    }

    #[test]
    fn toggle_power_flips_the_power() {
        let off = state(Power::Off, LightMode::Colour(colour(120, 800)));

        assert_eq!(off.apply(&BulbCommand::TogglePower).power, Power::On);
        assert_eq!(
            off.apply(&BulbCommand::TogglePower)
                .apply(&BulbCommand::TogglePower),
            off
        );
    }

    #[test]
    fn set_colour_switches_to_colour_mode_and_turns_on() {
        let off_white = state(Power::Off, white(500, 200));

        assert_eq!(
            off_white.apply(&BulbCommand::SetColour(colour(240, 700))),
            state(Power::On, LightMode::Colour(colour(240, 700)))
        );
    }

    #[test]
    fn set_white_switches_to_white_mode_and_turns_on() {
        let off_colour = state(Power::Off, LightMode::Colour(colour(240, 700)));
        let command = BulbCommand::SetWhite {
            brightness: brightness(300),
            temperature: temperature(900),
        };

        assert_eq!(
            off_colour.apply(&command),
            state(Power::On, white(300, 900))
        );
    }

    #[test]
    fn set_temperature_in_white_mode_keeps_the_brightness() {
        let on_white = state(Power::On, white(450, 100));

        assert_eq!(
            on_white.apply(&BulbCommand::SetTemperature(temperature(800))),
            state(Power::On, white(450, 800))
        );
    }

    #[test]
    fn set_temperature_from_colour_mode_keeps_the_colour_brightness() {
        let on_colour = state(Power::On, LightMode::Colour(colour(0, 350)));

        assert_eq!(
            on_colour.apply(&BulbCommand::SetTemperature(temperature(600))),
            state(Power::On, white(350, 600))
        );
    }

    #[test]
    fn set_temperature_from_a_scene_uses_full_brightness_and_turns_on() {
        let off_scene = state(Power::Off, LightMode::Scene);

        assert_eq!(
            off_scene.apply(&BulbCommand::SetTemperature(temperature(600))),
            state(Power::On, white(1000, 600))
        );
    }

    #[test]
    fn set_brightness_in_colour_mode_changes_only_the_colour_value() {
        let on_colour = state(Power::On, LightMode::Colour(colour(200, 900)));

        assert_eq!(
            on_colour.apply(&BulbCommand::SetBrightness(brightness(250))),
            state(Power::On, LightMode::Colour(colour(200, 250)))
        );
    }

    #[test]
    fn set_brightness_in_white_mode_changes_only_the_white_brightness() {
        let on_white = state(Power::On, white(900, 300));

        assert_eq!(
            on_white.apply(&BulbCommand::SetBrightness(brightness(120))),
            state(Power::On, white(120, 300))
        );
    }

    #[test]
    fn set_brightness_turns_the_bulb_on() {
        let off_white = state(Power::Off, white(900, 300));

        assert_eq!(
            off_white.apply(&BulbCommand::SetBrightness(brightness(120))),
            state(Power::On, white(120, 300))
        );
    }

    #[test]
    fn brightness_commands_in_a_scene_only_turn_the_bulb_on() {
        let off_scene = state(Power::Off, LightMode::Scene);
        let on_scene = state(Power::On, LightMode::Scene);

        assert_eq!(
            off_scene.apply(&BulbCommand::SetBrightness(brightness(500))),
            on_scene
        );
        assert_eq!(off_scene.apply(&BulbCommand::StepBrightness(10)), on_scene);
        assert_eq!(
            state(Power::Off, LightMode::Music).apply(&BulbCommand::StepBrightness(-10)),
            state(Power::On, LightMode::Music)
        );
    }

    #[test]
    fn step_brightness_adds_percentage_points() {
        // 655‰ aparece como 66%; +10 pontos = 76%.
        let on_colour = state(Power::On, LightMode::Colour(colour(10, 655)));

        assert_eq!(
            on_colour.apply(&BulbCommand::StepBrightness(10)),
            state(Power::On, LightMode::Colour(colour(10, 760)))
        );
        assert_eq!(
            state(Power::On, white(500, 0)).apply(&BulbCommand::StepBrightness(-25)),
            state(Power::On, white(250, 0))
        );
    }

    #[test]
    fn step_brightness_stays_between_1_and_100_percent() {
        assert_eq!(
            state(Power::On, white(950, 0)).apply(&BulbCommand::StepBrightness(10)),
            state(Power::On, white(1000, 0))
        );
        assert_eq!(
            state(Power::On, white(50, 0)).apply(&BulbCommand::StepBrightness(-10)),
            state(Power::On, white(10, 0))
        );
        assert_eq!(
            state(Power::On, white(10, 0)).apply(&BulbCommand::StepBrightness(i8::MIN)),
            state(Power::On, white(10, 0))
        );
    }

    #[test]
    fn step_brightness_never_turns_the_bulb_off_and_turns_it_on() {
        let off_minimum = state(Power::Off, white(10, 0));

        assert_eq!(
            off_minimum.apply(&BulbCommand::StepBrightness(-10)),
            state(Power::On, white(10, 0))
        );
    }
}
