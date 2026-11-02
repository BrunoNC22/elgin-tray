use crate::brightness::Brightness;
use crate::colour::{Hsv, Rgb};
use crate::error::DomainError;
use crate::temperature::ColorTemperature;

/// Identificador único da lâmpada (na Tuya, o Device ID).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BulbId(String);

impl BulbId {
    /// Cria o identificador, sem espaços nas pontas; recusa texto vazio.
    pub fn try_new(id: impl Into<String>) -> Result<Self, DomainError> {
        non_empty_trimmed(id.into(), "identificador da lâmpada").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Nome da lâmpada mostrado ao usuário (ex.: "Lâmpada do quarto").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BulbName(String);

impl BulbName {
    /// Cria o nome, sem espaços nas pontas; recusa texto vazio.
    pub fn try_new(name: impl Into<String>) -> Result<Self, DomainError> {
        non_empty_trimmed(name.into(), "nome da lâmpada").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn non_empty_trimmed(text: String, quantity: &'static str) -> Result<String, DomainError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Err(DomainError::Empty { quantity })
    } else if trimmed.len() == text.len() {
        Ok(text)
    } else {
        Ok(trimmed.to_owned())
    }
}

/// Lâmpada ligada ou desligada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Power {
    On,
    Off,
}

impl Power {
    pub fn is_on(self) -> bool {
        self == Self::On
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::On => Self::Off,
            Self::Off => Self::On,
        }
    }
}

/// O que a lâmpada está mostrando.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightMode {
    /// Luz branca com brilho e temperatura.
    White {
        brightness: Brightness,
        temperature: ColorTemperature,
    },
    /// Luz colorida; o brilho é o `value` do HSV.
    Colour(Hsv),
    /// Cena embutida da lâmpada. O app mostra, mas não edita.
    Scene,
    /// Modo música da lâmpada. O app mostra, mas não edita.
    Music,
}

impl LightMode {
    /// Brilho atual, quando o modo tem um (branco e cor).
    pub fn brightness(&self) -> Option<Brightness> {
        match self {
            Self::White { brightness, .. } => Some(*brightness),
            Self::Colour(hsv) => Some(hsv.value()),
            Self::Scene | Self::Music => None,
        }
    }

    /// Cor que representa o modo na interface, **na intensidade máxima** (sem aplicar o brilho).
    ///
    /// É a "cor cheia" do fim do gradiente do slider de brilho e a cor do ícone da bandeja.
    /// Cena e música não têm uma cor fixa: `None`.
    pub fn display_colour(&self) -> Option<Rgb> {
        match self {
            Self::White { temperature, .. } => Some(temperature.display_colour()),
            Self::Colour(hsv) => Some(hsv.with_value(Brightness::MAX).to_rgb()),
            Self::Scene | Self::Music => None,
        }
    }
}

/// Estado completo da lâmpada num instante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BulbState {
    pub power: Power,
    pub mode: LightMode,
}

/// Uma lâmpada com identidade, nome e estado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bulb {
    pub id: BulbId,
    pub name: BulbName,
    pub state: BulbState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colour::{Hue, Saturation};

    fn brightness(permille: u16) -> Brightness {
        Brightness::try_from_permille(permille).unwrap()
    }

    fn red(value: u16) -> Hsv {
        Hsv::new(Hue::new(0), Saturation::FULL, brightness(value))
    }

    #[test]
    fn bulb_id_trims_and_keeps_the_text() {
        assert_eq!(BulbId::try_new("  eb123abc ").unwrap().as_str(), "eb123abc");
    }

    #[test]
    fn bulb_id_rejects_empty_text() {
        assert_eq!(
            BulbId::try_new("   "),
            Err(DomainError::Empty {
                quantity: "identificador da lâmpada"
            })
        );
    }

    #[test]
    fn bulb_name_trims_and_rejects_empty_text() {
        assert_eq!(
            BulbName::try_new(" Lâmpada do quarto ").unwrap().as_str(),
            "Lâmpada do quarto"
        );
        assert_eq!(
            BulbName::try_new(""),
            Err(DomainError::Empty {
                quantity: "nome da lâmpada"
            })
        );
    }

    #[test]
    fn power_toggles() {
        assert!(Power::On.is_on());
        assert!(!Power::Off.is_on());
        assert_eq!(Power::On.toggled(), Power::Off);
        assert_eq!(Power::Off.toggled(), Power::On);
    }

    #[test]
    fn white_and_colour_modes_report_their_brightness() {
        let white = LightMode::White {
            brightness: brightness(300),
            temperature: ColorTemperature::WARMEST,
        };

        assert_eq!(white.brightness(), Some(brightness(300)));
        assert_eq!(
            LightMode::Colour(red(700)).brightness(),
            Some(brightness(700))
        );
        assert_eq!(LightMode::Scene.brightness(), None);
        assert_eq!(LightMode::Music.brightness(), None);
    }

    #[test]
    fn colour_mode_displays_the_full_intensity_colour() {
        let dim_red = LightMode::Colour(red(100));

        assert_eq!(dim_red.display_colour(), Some(Rgb::new(255, 0, 0)));
    }

    #[test]
    fn white_mode_displays_the_temperature_colour_regardless_of_brightness() {
        let dim = LightMode::White {
            brightness: Brightness::MIN,
            temperature: ColorTemperature::COOLEST,
        };

        assert_eq!(
            dim.display_colour(),
            Some(ColorTemperature::COOLEST.display_colour())
        );
    }

    #[test]
    fn scene_and_music_have_no_display_colour() {
        assert_eq!(LightMode::Scene.display_colour(), None);
        assert_eq!(LightMode::Music.display_colour(), None);
    }
}
