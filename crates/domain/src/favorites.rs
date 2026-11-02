use crate::brightness::Brightness;
use crate::bulb::{BulbState, LightMode};
use crate::colour::{Hsv, Hue, Saturation};
use crate::command::BulbCommand;
use crate::error::{DomainError, ensure_range};

/// Uma cor favorita: só matiz e saturação.
///
/// O brilho não faz parte da favorita: aplicar uma favorita mantém o brilho atual, para o atalho
/// "próxima favorita" não fazer o brilho pular.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FavoriteColour {
    hue: Hue,
    saturation: Saturation,
}

impl FavoriteColour {
    pub fn new(hue: Hue, saturation: Saturation) -> Self {
        Self { hue, saturation }
    }

    pub fn hue(self) -> Hue {
        self.hue
    }

    pub fn saturation(self) -> Saturation {
        self.saturation
    }

    /// A cor do modo atual, se o modo for cor (branco, cena e música não viram favorita).
    pub fn of(mode: &LightMode) -> Option<Self> {
        match mode {
            LightMode::Colour(hsv) => Some(Self::new(hsv.hue(), hsv.saturation())),
            LightMode::White { .. } | LightMode::Scene | LightMode::Music => None,
        }
    }

    /// Esta cor com o brilho dado.
    pub fn with_brightness(self, brightness: Brightness) -> Hsv {
        Hsv::new(self.hue, self.saturation, brightness)
    }
}

/// Posição de uma favorita na linha do painel (0 a 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FavoriteSlot(u8);

impl FavoriteSlot {
    pub fn try_new(index: u8) -> Result<Self, DomainError> {
        let last = Favorites::COUNT as u32 - 1;
        ensure_range("posição da favorita", index.into(), 0, last)?;
        Ok(Self(index))
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// Todas as posições, em ordem.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..Favorites::COUNT as u8).map(Self)
    }
}

/// As 6 cores favoritas do usuário.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Favorites {
    colours: [FavoriteColour; Favorites::COUNT],
}

impl Favorites {
    pub const COUNT: usize = 6;

    pub fn new(colours: [FavoriteColour; Self::COUNT]) -> Self {
        Self { colours }
    }

    pub fn get(&self, slot: FavoriteSlot) -> FavoriteColour {
        self.colours[slot.index()]
    }

    /// Guarda `colour` na posição `slot`, substituindo a anterior.
    pub fn save(&mut self, slot: FavoriteSlot, colour: FavoriteColour) {
        self.colours[slot.index()] = colour;
    }

    pub fn colours(&self) -> &[FavoriteColour; Self::COUNT] {
        &self.colours
    }

    /// Comando que aplica a favorita `slot` mantendo o brilho atual (ou 100%, se o modo não tem brilho).
    pub fn apply_command(&self, slot: FavoriteSlot, current: &BulbState) -> BulbCommand {
        let brightness = current.mode.brightness().unwrap_or(Brightness::MAX);
        BulbCommand::SetColour(self.get(slot).with_brightness(brightness))
    }

    /// Próxima favorita depois da cor atual, em ciclo. Se a cor atual não é uma favorita
    /// (ou a lâmpada não está no modo cor), começa pela primeira.
    pub fn next_slot(&self, current: &BulbState) -> FavoriteSlot {
        let next_index = FavoriteColour::of(&current.mode)
            .and_then(|colour| self.colours.iter().position(|&saved| saved == colour))
            .map_or(0, |index| (index + 1) % Self::COUNT);
        FavoriteSlot(next_index as u8)
    }
}

impl Default for Favorites {
    /// Vermelho, laranja, amarelo, verde, azul e roxo, todos com saturação máxima.
    fn default() -> Self {
        Self::new(
            [0, 30, 60, 120, 240, 280]
                .map(|hue| FavoriteColour::new(Hue::new(hue), Saturation::FULL)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bulb::Power;
    use crate::temperature::ColorTemperature;

    fn favourite(hue: u16) -> FavoriteColour {
        FavoriteColour::new(Hue::new(hue), Saturation::FULL)
    }

    fn slot(index: u8) -> FavoriteSlot {
        FavoriteSlot::try_new(index).unwrap()
    }

    fn brightness(permille: u16) -> Brightness {
        Brightness::try_from_permille(permille).unwrap()
    }

    fn colour_state(hue: u16, value: u16) -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::Colour(favourite(hue).with_brightness(brightness(value))),
        }
    }

    fn white_state() -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::White {
                brightness: brightness(400),
                temperature: ColorTemperature::WARMEST,
            },
        }
    }

    #[test]
    fn slots_go_from_0_to_5() {
        assert_eq!(slot(0).index(), 0);
        assert_eq!(slot(5).index(), 5);
        assert_eq!(
            FavoriteSlot::try_new(6),
            Err(DomainError::OutOfRange {
                quantity: "posição da favorita",
                value: 6,
                min: 0,
                max: 5
            })
        );
        assert_eq!(
            FavoriteSlot::all()
                .map(FavoriteSlot::index)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn defaults_are_six_saturated_rainbow_colours() {
        let hues: Vec<u16> = Favorites::default()
            .colours()
            .iter()
            .map(|colour| colour.hue().degrees())
            .collect();

        assert_eq!(hues, [0, 30, 60, 120, 240, 280]);
        assert!(
            Favorites::default()
                .colours()
                .iter()
                .all(|colour| colour.saturation() == Saturation::FULL)
        );
    }

    #[test]
    fn save_replaces_only_the_chosen_slot() {
        let mut favorites = Favorites::default();
        let teal = FavoriteColour::new(Hue::new(170), Saturation::try_from_permille(600).unwrap());

        favorites.save(slot(2), teal);

        assert_eq!(favorites.get(slot(2)), teal);
        assert_eq!(favorites.get(slot(1)), Favorites::default().get(slot(1)));
        assert_eq!(favorites.get(slot(3)), Favorites::default().get(slot(3)));
    }

    #[test]
    fn favourite_of_a_colour_mode_drops_the_brightness() {
        let dim_blue = colour_state(240, 120).mode;

        assert_eq!(FavoriteColour::of(&dim_blue), Some(favourite(240)));
    }

    #[test]
    fn white_scene_and_music_are_not_favourites() {
        assert_eq!(FavoriteColour::of(&white_state().mode), None);
        assert_eq!(FavoriteColour::of(&LightMode::Scene), None);
        assert_eq!(FavoriteColour::of(&LightMode::Music), None);
    }

    #[test]
    fn applying_a_favourite_keeps_the_current_brightness() {
        let favorites = Favorites::default();

        assert_eq!(
            favorites.apply_command(slot(4), &colour_state(0, 300)),
            BulbCommand::SetColour(favourite(240).with_brightness(brightness(300)))
        );
        assert_eq!(
            favorites.apply_command(slot(4), &white_state()),
            BulbCommand::SetColour(favourite(240).with_brightness(brightness(400)))
        );
    }

    #[test]
    fn applying_a_favourite_in_a_scene_uses_full_brightness() {
        let scene = BulbState {
            power: Power::Off,
            mode: LightMode::Scene,
        };

        assert_eq!(
            Favorites::default().apply_command(slot(0), &scene),
            BulbCommand::SetColour(favourite(0).with_brightness(Brightness::MAX))
        );
    }

    #[test]
    fn next_slot_follows_the_current_favourite() {
        let favorites = Favorites::default();

        assert_eq!(favorites.next_slot(&colour_state(60, 500)), slot(3));
    }

    #[test]
    fn next_slot_wraps_around_after_the_last() {
        assert_eq!(
            Favorites::default().next_slot(&colour_state(280, 500)),
            slot(0)
        );
    }

    #[test]
    fn next_slot_starts_from_the_first_when_the_colour_is_not_a_favourite() {
        let favorites = Favorites::default();

        assert_eq!(favorites.next_slot(&colour_state(200, 500)), slot(0));
        assert_eq!(favorites.next_slot(&white_state()), slot(0));
    }
}
