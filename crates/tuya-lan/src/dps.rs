//! Tradução entre os DPs (data points) da lâmpada e o estado do domínio.
//!
//! DPs da lâmpada Tuya de categoria `dj` (confirmados na lâmpada Elgin, ver CLAUDE.md):
//! `20` liga/desliga, `21` modo (`white`/`colour`/`scene`/`music`), `22` brilho do branco (10–1000),
//! `23` temperatura (0–1000), `24` cor `HHHHSSSSVVVV` em hex (H 0–360, S e V 0–1000).

use std::fmt;

use elgin_domain::{
    Brightness, BulbState, ColorTemperature, Hsv, Hue, LightMode, Power, Saturation,
};
use serde_json::{Map, Value};

/// Quais números de DP a lâmpada usa para cada coisa (outras lâmpadas podem usar outros).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DpMap {
    pub(crate) power: &'static str,
    pub(crate) mode: &'static str,
    pub(crate) white_brightness: &'static str,
    pub(crate) temperature: &'static str,
    pub(crate) colour: &'static str,
}

impl Default for DpMap {
    /// DPs das lâmpadas Tuya de categoria `dj` (a Elgin Smart Color usa estes).
    fn default() -> Self {
        Self {
            power: "20",
            mode: "21",
            white_brightness: "22",
            temperature: "23",
            colour: "24",
        }
    }
}

/// DP ausente ou com valor que não dá para entender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DpsError {
    Missing(&'static str),
    Invalid { dp: &'static str, value: String },
}

impl fmt::Display for DpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(dp) => write!(f, "a lâmpada não informou o DP {dp}"),
            Self::Invalid { dp, value } => write!(f, "valor inválido no DP {dp}: {value}"),
        }
    }
}

impl std::error::Error for DpsError {}

/// Os DPs que a lâmpada já informou (resposta de status + pushes), crus.
///
/// Guardar os valores crus permite, por exemplo, voltar ao modo cor com a última cor conhecida
/// mesmo que o push que mudou o modo não traga o DP da cor.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct KnownDps {
    values: Map<String, Value>,
}

impl KnownDps {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Junta DPs novos (um status completo ou um push parcial), sobrescrevendo os antigos.
    pub(crate) fn merge(&mut self, dps: &Map<String, Value>) {
        for (dp, value) in dps {
            self.values.insert(dp.clone(), value.clone());
        }
    }

    /// O estado da lâmpada segundo os DPs conhecidos. Valores fora da faixa são limitados.
    pub(crate) fn to_state(&self, map: &DpMap) -> Result<BulbState, DpsError> {
        let power = match self.get(map.power)? {
            Value::Bool(true) => Power::On,
            Value::Bool(false) => Power::Off,
            other => return Err(invalid(map.power, other)),
        };
        let mode_value = self.get(map.mode)?;
        let mode = match mode_value.as_str() {
            Some("white") => LightMode::White {
                brightness: Brightness::clamped_from_permille(self.number(map.white_brightness)?),
                temperature: ColorTemperature::clamped_from_permille(self.number(map.temperature)?),
            },
            Some("colour") => {
                let value = self.get(map.colour)?;
                let hsv = value
                    .as_str()
                    .and_then(decode_colour)
                    .ok_or_else(|| invalid(map.colour, value))?;
                LightMode::Colour(hsv)
            }
            Some("music") => LightMode::Music,
            // Cena e qualquer modo que o app não conhece: mostrado, mas não editável.
            Some(_) => LightMode::Scene,
            None => return Err(invalid(map.mode, mode_value)),
        };
        Ok(BulbState { power, mode })
    }

    /// DPs a enviar para deixar a lâmpada em `target`.
    ///
    /// - Desligar manda só o liga/desliga.
    /// - Liga/desliga e modo só vão quando mudam (trocar de modo à toa pode piscar a lâmpada).
    /// - Os valores do modo (brilho/temperatura ou cor) sempre vão.
    /// - Cena e música não têm valores editáveis: só o liga/desliga.
    pub(crate) fn dps_for(&self, target: &BulbState, map: &DpMap) -> Map<String, Value> {
        let mut dps = Map::new();
        self.insert_if_changed(&mut dps, map.power, Value::Bool(target.power.is_on()));
        if !target.power.is_on() {
            return dps;
        }
        match target.mode {
            LightMode::White {
                brightness,
                temperature,
            } => {
                self.insert_if_changed(&mut dps, map.mode, Value::from("white"));
                dps.insert(map.white_brightness.into(), brightness.permille().into());
                dps.insert(map.temperature.into(), temperature.permille().into());
            }
            LightMode::Colour(hsv) => {
                self.insert_if_changed(&mut dps, map.mode, Value::from("colour"));
                dps.insert(map.colour.into(), encode_colour(hsv).into());
            }
            LightMode::Scene | LightMode::Music => {}
        }
        dps
    }

    fn get(&self, dp: &'static str) -> Result<&Value, DpsError> {
        self.values.get(dp).ok_or(DpsError::Missing(dp))
    }

    /// Um DP numérico, limitado a 0..=u16::MAX (as faixas do domínio limitam de novo depois).
    fn number(&self, dp: &'static str) -> Result<u16, DpsError> {
        let value = self.get(dp)?;
        let number = value.as_i64().ok_or_else(|| invalid(dp, value))?;
        Ok(number.clamp(0, i64::from(u16::MAX)) as u16)
    }

    fn insert_if_changed(&self, dps: &mut Map<String, Value>, dp: &'static str, value: Value) {
        if self.values.get(dp) != Some(&value) {
            dps.insert(dp.into(), value);
        }
    }
}

fn invalid(dp: &'static str, value: &Value) -> DpsError {
    DpsError::Invalid {
        dp,
        value: value.to_string(),
    }
}

/// Cor no formato do DP 24: `HHHHSSSSVVVV` em hexadecimal.
pub(crate) fn encode_colour(hsv: Hsv) -> String {
    format!(
        "{:04x}{:04x}{:04x}",
        hsv.hue().degrees(),
        hsv.saturation().permille(),
        hsv.value().permille()
    )
}

/// Lê o formato do DP 24, limitando os valores às faixas do domínio.
pub(crate) fn decode_colour(text: &str) -> Option<Hsv> {
    if text.len() != 12 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let part = |index: usize| u16::from_str_radix(&text[index * 4..index * 4 + 4], 16).ok();
    Some(Hsv::new(
        Hue::new(part(0)?),
        Saturation::clamped_from_permille(part(1)?),
        Brightness::clamped_from_permille(part(2)?),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dps(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn known(value: Value) -> KnownDps {
        let mut known = KnownDps::new();
        known.merge(&dps(value));
        known
    }

    fn state(value: Value) -> Result<BulbState, DpsError> {
        known(value).to_state(&DpMap::default())
    }

    fn white(brightness: u16, temperature: u16) -> LightMode {
        LightMode::White {
            brightness: Brightness::try_from_permille(brightness).unwrap(),
            temperature: ColorTemperature::try_from_permille(temperature).unwrap(),
        }
    }

    fn colour(hue: u16, saturation: u16, value: u16) -> Hsv {
        Hsv::new(
            Hue::new(hue),
            Saturation::try_from_permille(saturation).unwrap(),
            Brightness::try_from_permille(value).unwrap(),
        )
    }

    fn on(mode: LightMode) -> BulbState {
        BulbState {
            power: Power::On,
            mode,
        }
    }

    /// O status real lido da lâmpada do usuário (CLAUDE.md).
    fn real_status() -> Value {
        json!({"20": true, "21": "white", "22": 1000, "23": 1000, "24": "000003e803e8",
               "25": "184646020028032003e800000000", "26": 0, "34": false, "41": true})
    }

    #[test]
    fn default_map_uses_the_dj_category_numbers() {
        let map = DpMap::default();

        assert_eq!(
            [
                map.power,
                map.mode,
                map.white_brightness,
                map.temperature,
                map.colour
            ],
            ["20", "21", "22", "23", "24"]
        );
    }

    #[test]
    fn reads_the_real_status_of_the_bulb() {
        assert_eq!(state(real_status()), Ok(on(white(1000, 1000))));
    }

    #[test]
    fn reads_colour_mode() {
        let status = json!({"20": true, "21": "colour", "24": "00f003e801f4"});

        assert_eq!(
            state(status),
            Ok(on(LightMode::Colour(colour(240, 1000, 500))))
        );
    }

    #[test]
    fn reads_power_off() {
        let status = json!({"20": false, "21": "white", "22": 300, "23": 0});

        assert_eq!(state(status).unwrap().power, Power::Off);
    }

    #[test]
    fn reads_scene_music_and_unknown_modes() {
        assert_eq!(
            state(json!({"20": true, "21": "scene"})).unwrap().mode,
            LightMode::Scene
        );
        assert_eq!(
            state(json!({"20": true, "21": "music"})).unwrap().mode,
            LightMode::Music
        );
        assert_eq!(
            state(json!({"20": true, "21": "disco"})).unwrap().mode,
            LightMode::Scene
        );
    }

    #[test]
    fn a_partial_push_updates_only_what_it_brings() {
        let mut known = known(real_status());

        known.merge(&dps(json!({"22": 500})));

        assert_eq!(known.to_state(&DpMap::default()), Ok(on(white(500, 1000))));
    }

    #[test]
    fn switching_to_colour_without_the_colour_dp_uses_the_last_known_colour() {
        let mut known = known(real_status());

        known.merge(&dps(json!({"21": "colour"})));

        assert_eq!(
            known.to_state(&DpMap::default()),
            Ok(on(LightMode::Colour(colour(0, 1000, 1000))))
        );
    }

    #[test]
    fn missing_dps_are_reported() {
        assert_eq!(state(json!({"21": "white"})), Err(DpsError::Missing("20")));
        assert_eq!(state(json!({"20": true})), Err(DpsError::Missing("21")));
        assert_eq!(
            state(json!({"20": true, "21": "white", "23": 0})),
            Err(DpsError::Missing("22"))
        );
        assert_eq!(
            state(json!({"20": true, "21": "colour"})),
            Err(DpsError::Missing("24"))
        );
    }

    #[test]
    fn values_of_the_wrong_type_are_invalid() {
        assert!(matches!(
            state(json!({"20": "sim", "21": "white", "22": 10, "23": 0})),
            Err(DpsError::Invalid { dp: "20", .. })
        ));
        assert!(matches!(
            state(json!({"20": true, "21": "white", "22": "muito", "23": 0})),
            Err(DpsError::Invalid { dp: "22", .. })
        ));
        assert!(matches!(
            state(json!({"20": true, "21": "colour", "24": "zz"})),
            Err(DpsError::Invalid { dp: "24", .. })
        ));
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        assert_eq!(
            state(json!({"20": true, "21": "white", "22": 5, "23": 2000})),
            Ok(on(white(10, 1000)))
        );
        assert_eq!(
            state(json!({"20": true, "21": "white", "22": -3, "23": -1})),
            Ok(on(white(10, 0)))
        );
        assert_eq!(
            state(json!({"20": true, "21": "colour", "24": "0190ffff0000"})),
            Ok(on(LightMode::Colour(colour(40, 1000, 10))))
        );
    }

    #[test]
    fn encodes_and_decodes_the_colour_format() {
        assert_eq!(encode_colour(colour(0, 1000, 1000)), "000003e803e8");
        assert_eq!(encode_colour(colour(240, 1000, 500)), "00f003e801f4");
        assert_eq!(decode_colour("00f003e801f4"), Some(colour(240, 1000, 500)));
        assert_eq!(decode_colour("00F003E801F4"), Some(colour(240, 1000, 500)));
        assert_eq!(decode_colour("00f003e801"), None);
        assert_eq!(decode_colour("00f003e801f4ff"), None);
    }

    #[test]
    fn turning_off_sends_only_the_power() {
        let target = BulbState {
            power: Power::Off,
            mode: white(1000, 1000),
        };

        assert_eq!(
            known(real_status()).dps_for(&target, &DpMap::default()),
            dps(json!({"20": false}))
        );
    }

    #[test]
    fn changing_the_white_brightness_sends_only_the_white_values() {
        let target = on(white(250, 1000));

        assert_eq!(
            known(real_status()).dps_for(&target, &DpMap::default()),
            dps(json!({"22": 250, "23": 1000}))
        );
    }

    #[test]
    fn switching_to_colour_sends_the_mode_and_the_colour() {
        let target = on(LightMode::Colour(colour(120, 800, 600)));

        assert_eq!(
            known(real_status()).dps_for(&target, &DpMap::default()),
            dps(json!({"21": "colour", "24": "007803200258"}))
        );
    }

    #[test]
    fn turning_on_with_a_new_value_sends_the_power_too() {
        let off = known(json!({"20": false, "21": "white", "22": 300, "23": 0}));

        assert_eq!(
            off.dps_for(&on(white(300, 0)), &DpMap::default()),
            dps(json!({"20": true, "22": 300, "23": 0}))
        );
    }

    #[test]
    fn without_known_dps_everything_is_sent() {
        assert_eq!(
            KnownDps::new().dps_for(&on(white(300, 0)), &DpMap::default()),
            dps(json!({"20": true, "21": "white", "22": 300, "23": 0}))
        );
    }

    #[test]
    fn scene_and_music_targets_only_switch_the_power() {
        let off_scene = known(json!({"20": false, "21": "scene"}));

        assert_eq!(
            off_scene.dps_for(&on(LightMode::Scene), &DpMap::default()),
            dps(json!({"20": true}))
        );
        assert_eq!(
            off_scene.dps_for(&on(LightMode::Music), &DpMap::default()),
            dps(json!({"20": true}))
        );
    }
}
