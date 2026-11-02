//! Atalhos globais: o texto do `config.toml` (`Ctrl+Alt+L`), o registro no sistema e o comando
//! que cada atalho manda para a lâmpada.

use std::fmt;
use std::str::FromStr;

use elgin_domain::{BulbCommand, Favorites};

use crate::config::HotkeyBindings;
use crate::ports::HotkeyRegistrar;
use crate::status::{BulbSnapshot, ConnectionStatus};

/// Pontos percentuais de brilho por atalho de brilho.
pub const HOTKEY_STEP_PERCENT: i8 = 10;

/// O que um atalho faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyAction {
    Toggle,
    BrightnessUp,
    BrightnessDown,
    NextFavorite,
}

impl HotkeyAction {
    pub const ALL: [Self; 4] = [
        Self::Toggle,
        Self::BrightnessUp,
        Self::BrightnessDown,
        Self::NextFavorite,
    ];

    /// Nome para mensagens ao usuário.
    pub fn label(self) -> &'static str {
        match self {
            Self::Toggle => "ligar/desligar",
            Self::BrightnessUp => "aumentar o brilho",
            Self::BrightnessDown => "diminuir o brilho",
            Self::NextFavorite => "próxima favorita",
        }
    }
}

/// Teclas modificadoras de um atalho.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Tecla Windows / Super / Meta.
    pub super_key: bool,
}

impl Modifiers {
    fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.super_key
    }
}

/// A tecla principal de um atalho.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// `A`–`Z` (sempre maiúscula).
    Letter(char),
    /// `0`–`9`.
    Digit(u8),
    /// `F1`–`F24`.
    Function(u8),
    Up,
    Down,
    Left,
    Right,
    Space,
    Enter,
    Tab,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
}

const NAMED_KEYS: [(&str, Key); 12] = [
    ("Up", Key::Up),
    ("Down", Key::Down),
    ("Left", Key::Left),
    ("Right", Key::Right),
    ("Space", Key::Space),
    ("Enter", Key::Enter),
    ("Tab", Key::Tab),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PageUp", Key::PageUp),
    ("PageDown", Key::PageDown),
    ("Insert", Key::Insert),
];

impl Key {
    fn parse(text: &str) -> Option<Self> {
        let upper = text.to_ascii_uppercase();
        let mut chars = upper.chars();
        if let (Some(single), None) = (chars.next(), chars.next()) {
            return match single {
                'A'..='Z' => Some(Self::Letter(single)),
                '0'..='9' => Some(Self::Digit(single as u8 - b'0')),
                _ => None,
            };
        }
        if let Some(number) = upper.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
            return (1..=24).contains(&number).then_some(Self::Function(number));
        }
        if upper == "DELETE" || upper == "DEL" {
            return Some(Self::Delete);
        }
        NAMED_KEYS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(text))
            .map(|&(_, key)| key)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Letter(letter) => write!(f, "{letter}"),
            Self::Digit(digit) => write!(f, "{digit}"),
            Self::Function(number) => write!(f, "F{number}"),
            Self::Delete => f.write_str("Delete"),
            other => {
                let name = NAMED_KEYS
                    .iter()
                    .find(|(_, key)| key == other)
                    .map_or("?", |&(name, _)| name);
                f.write_str(name)
            }
        }
    }
}

/// Um atalho: modificadoras + uma tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeyBinding {
    pub modifiers: Modifiers,
    pub key: Key,
}

/// Por que o texto de um atalho não vale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    UnknownKey(String),
    MissingKey,
    MultipleKeys,
    /// Sem Ctrl, Alt, Shift ou Win o atalho roubaria a tecla de todos os programas.
    NoModifier,
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(f, "tecla desconhecida \"{key}\""),
            Self::MissingKey => f.write_str("falta a tecla principal (ex.: Ctrl+Alt+L)"),
            Self::MultipleKeys => f.write_str("só pode haver uma tecla além das modificadoras"),
            Self::NoModifier => {
                f.write_str("use pelo menos uma modificadora (Ctrl, Alt, Shift ou Win)")
            }
        }
    }
}

impl std::error::Error for HotkeyError {}

impl FromStr for HotkeyBinding {
    type Err = HotkeyError;

    /// Partes separadas por `+`, sem diferença entre maiúsculas e minúsculas: `Ctrl+Alt+L`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "win" | "super" | "meta" => modifiers.super_key = true,
                "" => return Err(HotkeyError::MissingKey),
                _ => {
                    let parsed =
                        Key::parse(part).ok_or_else(|| HotkeyError::UnknownKey(part.to_owned()))?;
                    if key.replace(parsed).is_some() {
                        return Err(HotkeyError::MultipleKeys);
                    }
                }
            }
        }
        let key = key.ok_or(HotkeyError::MissingKey)?;
        if !modifiers.any() {
            return Err(HotkeyError::NoModifier);
        }
        Ok(Self { modifiers, key })
    }
}

impl fmt::Display for HotkeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Modifiers {
            ctrl,
            alt,
            shift,
            super_key,
        } = self.modifiers;
        for (on, name) in [
            (ctrl, "Ctrl"),
            (alt, "Alt"),
            (shift, "Shift"),
            (super_key, "Win"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

impl HotkeyBindings {
    /// O texto configurado para cada ação (vazio = atalho desligado).
    pub fn text(&self, action: HotkeyAction) -> &str {
        match action {
            HotkeyAction::Toggle => &self.toggle,
            HotkeyAction::BrightnessUp => &self.brightness_up,
            HotkeyAction::BrightnessDown => &self.brightness_down,
            HotkeyAction::NextFavorite => &self.next_favorite,
        }
    }
}

/// Registra os atalhos do config. Devolve uma mensagem para cada atalho que ficou de fora (texto
/// inválido, repetido ou já usado por outro programa); o resto funciona normalmente.
pub fn register_hotkeys<H: HotkeyRegistrar>(
    bindings: &HotkeyBindings,
    registrar: &mut H,
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut valid: Vec<(HotkeyAction, HotkeyBinding)> = Vec::new();
    for action in HotkeyAction::ALL {
        let text = bindings.text(action).trim();
        if text.is_empty() {
            continue;
        }
        match text.parse::<HotkeyBinding>() {
            Ok(binding) if valid.iter().any(|&(_, used)| used == binding) => {
                problems.push(format!(
                    "atalho de {}: {binding} já é usado por outra ação",
                    action.label()
                ))
            }
            Ok(binding) => valid.push((action, binding)),
            Err(error) => problems.push(format!(
                "atalho de {} (\"{text}\"): {error}",
                action.label()
            )),
        }
    }
    for (action, reason) in registrar.register(&valid) {
        let binding = valid
            .iter()
            .find(|&&(registered, _)| registered == action)
            .map(|(_, binding)| binding.to_string())
            .unwrap_or_default();
        problems.push(format!(
            "atalho de {} ({binding}) não registrado: {reason}",
            action.label()
        ));
    }
    problems
}

/// O comando que um atalho manda (`None` se a lâmpada não está disponível).
pub fn hotkey_command(
    action: HotkeyAction,
    snapshot: &BulbSnapshot,
    favorites: &Favorites,
) -> Option<BulbCommand> {
    let state = snapshot
        .state
        .filter(|_| snapshot.connection == ConnectionStatus::Online)?;
    Some(match action {
        HotkeyAction::Toggle => BulbCommand::TogglePower,
        HotkeyAction::BrightnessUp => BulbCommand::StepBrightness(HOTKEY_STEP_PERCENT),
        HotkeyAction::BrightnessDown => BulbCommand::StepBrightness(-HOTKEY_STEP_PERCENT),
        HotkeyAction::NextFavorite => favorites.apply_command(favorites.next_slot(&state), &state),
    })
}

#[cfg(test)]
mod tests {
    use elgin_domain::{
        Brightness, BulbState, FavoriteSlot, Hsv, Hue, LightMode, Power, Saturation,
    };

    use super::*;
    use crate::testing::SpyHotkeys;

    fn binding(text: &str) -> HotkeyBinding {
        text.parse().unwrap()
    }

    #[test]
    fn parses_modifiers_and_key_in_any_case() {
        assert_eq!(
            binding("Ctrl+Alt+L"),
            HotkeyBinding {
                modifiers: Modifiers {
                    ctrl: true,
                    alt: true,
                    ..Modifiers::default()
                },
                key: Key::Letter('L'),
            }
        );
        assert_eq!(binding(" control + SHIFT + l "), binding("Ctrl+Shift+L"));
        assert_eq!(binding("Win+F12").key, Key::Function(12));
        assert!(binding("super+1").modifiers.super_key);
        assert_eq!(binding("Ctrl+Alt+pageup").key, Key::PageUp);
        assert_eq!(binding("Ctrl+Del").key, Key::Delete);
    }

    #[test]
    fn displays_in_canonical_form() {
        for text in [
            "Ctrl+Alt+L",
            "Ctrl+Alt+Up",
            "Shift+Win+F5",
            "Alt+7",
            "Ctrl+Delete",
        ] {
            assert_eq!(binding(text).to_string(), text);
        }
        assert_eq!(binding("alt+ctrl+down").to_string(), "Ctrl+Alt+Down");
    }

    #[test]
    fn rejects_invalid_text_with_a_clear_reason() {
        let cases = [
            ("Ctrl+Alt", HotkeyError::MissingKey),
            ("Ctrl+", HotkeyError::MissingKey),
            ("Ctrl+Alt+Foo", HotkeyError::UnknownKey("Foo".to_owned())),
            ("Ctrl+F25", HotkeyError::UnknownKey("F25".to_owned())),
            ("Ctrl+A+B", HotkeyError::MultipleKeys),
            ("L", HotkeyError::NoModifier),
        ];
        for (text, error) in cases {
            assert_eq!(text.parse::<HotkeyBinding>(), Err(error), "{text}");
        }
        assert!(HotkeyError::NoModifier.to_string().contains("modificadora"));
    }

    #[test]
    fn registers_the_default_bindings() {
        let mut registrar = SpyHotkeys::new();

        let problems = register_hotkeys(&HotkeyBindings::default(), &mut registrar);

        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            registrar.registered(),
            [
                (HotkeyAction::Toggle, binding("Ctrl+Alt+L")),
                (HotkeyAction::BrightnessUp, binding("Ctrl+Alt+Up")),
                (HotkeyAction::BrightnessDown, binding("Ctrl+Alt+Down")),
                (HotkeyAction::NextFavorite, binding("Ctrl+Alt+Right")),
            ]
        );
    }

    #[test]
    fn invalid_repeated_and_taken_bindings_are_reported_and_the_rest_still_works() {
        let bindings = HotkeyBindings {
            toggle: "Ctrl+Alt+Nope".to_owned(),
            brightness_up: "Ctrl+Alt+Up".to_owned(),
            brightness_down: "ctrl+alt+up".to_owned(),
            next_favorite: "Ctrl+Alt+Right".to_owned(),
        };
        let mut registrar = SpyHotkeys::new();
        registrar.reject(binding("Ctrl+Alt+Right"), "já usado por outro programa");

        let problems = register_hotkeys(&bindings, &mut registrar);

        assert_eq!(
            problems,
            [
                "atalho de ligar/desligar (\"Ctrl+Alt+Nope\"): tecla desconhecida \"Nope\"",
                "atalho de diminuir o brilho: Ctrl+Alt+Up já é usado por outra ação",
                "atalho de próxima favorita (Ctrl+Alt+Right) não registrado: já usado por outro programa",
            ]
        );
        assert_eq!(
            registrar.registered(),
            [(HotkeyAction::BrightnessUp, binding("Ctrl+Alt+Up"))]
        );
    }

    #[test]
    fn an_empty_binding_turns_the_hotkey_off() {
        let bindings = HotkeyBindings {
            toggle: " ".to_owned(),
            ..HotkeyBindings::default()
        };
        let mut registrar = SpyHotkeys::new();

        assert!(register_hotkeys(&bindings, &mut registrar).is_empty());
        assert_eq!(registrar.registered().len(), 3);
    }

    fn online(state: BulbState) -> BulbSnapshot {
        BulbSnapshot {
            state: Some(state),
            connection: ConnectionStatus::Online,
        }
    }

    fn colour(hue: u16) -> BulbState {
        BulbState {
            power: Power::On,
            mode: LightMode::Colour(Hsv::new(
                Hue::new(hue),
                Saturation::FULL,
                Brightness::from_percent(50).unwrap(),
            )),
        }
    }

    #[test]
    fn each_action_sends_its_command() {
        let snapshot = online(colour(0));
        let favorites = Favorites::default();
        let command = |action| hotkey_command(action, &snapshot, &favorites);

        assert_eq!(
            command(HotkeyAction::Toggle),
            Some(BulbCommand::TogglePower)
        );
        assert_eq!(
            command(HotkeyAction::BrightnessUp),
            Some(BulbCommand::StepBrightness(10))
        );
        assert_eq!(
            command(HotkeyAction::BrightnessDown),
            Some(BulbCommand::StepBrightness(-10))
        );
        // A cor atual (vermelho) é a primeira favorita: vai para a segunda, com o mesmo brilho.
        let second = favorites.get(FavoriteSlot::try_new(1).unwrap());
        assert_eq!(
            command(HotkeyAction::NextFavorite),
            Some(BulbCommand::SetColour(
                second.with_brightness(Brightness::from_percent(50).unwrap())
            ))
        );
    }

    #[test]
    fn nothing_is_sent_while_the_bulb_is_unavailable() {
        let offline = BulbSnapshot {
            connection: ConnectionStatus::Connecting,
            ..online(colour(0))
        };

        assert_eq!(
            hotkey_command(HotkeyAction::Toggle, &offline, &Favorites::default()),
            None
        );
    }
}
