//! Argumentos da linha de comando.

use elgin_domain::{Brightness, BulbCommand, ColorTemperature, Hsv, Hue, Power, Saturation};
use elgin_panel::placement::Rect;

pub(crate) const USAGE: &str = "\
Uso:
  elgin-tray                         inicia o app na bandeja
  elgin-tray ctl <comando>           controla a lâmpada pelo terminal (desenvolvimento)

Comandos do ctl:
  status                             mostra o estado da lâmpada
  on | off | toggle                  liga, desliga ou alterna
  color <matiz> <saturação> <brilho> cor: matiz 0-359°, saturação 0-100%, brilho 1-100%
  white <brilho> <temperatura>       branco: brilho 1-100%, temperatura 0 (quente) a 100 (fria)
  brightness <brilho>                muda o brilho do modo atual (1-100%)
  discover                           procura a lâmpada na rede e mostra o IP
  watch                              mostra as mudanças da lâmpada até ser interrompido (Ctrl+C)
  login <código de usuário>          login por QR code no Smart Life (guarda a sessão da nuvem)
  devices                            lista os dispositivos da conta na nuvem (precisa do login)

A configuração é lida de %APPDATA%\\elgin-tray\\config.toml (Windows) ou
~/.config/elgin-tray/config.toml (Linux); para usar outro arquivo, defina ELGIN_TRAY_CONFIG.";

/// O que o executável deve fazer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Daemon,
    /// O painel, com a posição do ícone da bandeja (se veio).
    Panel(Option<Rect>),
    Ctl(CtlCommand),
    Help,
}

/// Um comando do `elgin-tray ctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CtlCommand {
    Status,
    Discover,
    Watch,
    /// Login por QR code com o código de usuário do Smart Life.
    Login(String),
    /// Lista os dispositivos da conta na nuvem.
    Devices,
    /// Um comando para a lâmpada.
    Apply(BulbCommand),
}

/// Lê os argumentos (sem o nome do programa). O erro é uma mensagem para o usuário.
pub(crate) fn parse(args: &[String]) -> Result<Mode, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => Ok(Mode::Daemon),
        // Uma âncora malformada não impede o painel de abrir (só não fica colado no ícone).
        ["panel"] => Ok(Mode::Panel(None)),
        ["panel", "--anchor", anchor] => Ok(Mode::Panel(parse_anchor(anchor))),
        ["help" | "--help" | "-h"] => Ok(Mode::Help),
        ["ctl", rest @ ..] => parse_ctl(rest).map(Mode::Ctl),
        _ => Err(format!("argumentos desconhecidos: {}", words.join(" "))),
    }
}

fn parse_ctl(words: &[&str]) -> Result<CtlCommand, String> {
    let apply = |command| Ok(CtlCommand::Apply(command));
    match words {
        ["status"] => Ok(CtlCommand::Status),
        ["discover"] => Ok(CtlCommand::Discover),
        ["watch"] => Ok(CtlCommand::Watch),
        ["login", user_code] => Ok(CtlCommand::Login((*user_code).to_owned())),
        ["devices"] => Ok(CtlCommand::Devices),
        ["on"] => apply(BulbCommand::SetPower(Power::On)),
        ["off"] => apply(BulbCommand::SetPower(Power::Off)),
        ["toggle"] => apply(BulbCommand::TogglePower),
        ["color", hue, saturation, brightness] => {
            let hue = number("matiz", hue, 0, 359)?;
            let saturation = number("saturação", saturation, 0, 100)?;
            let brightness = number("brilho", brightness, 1, 100)?;
            apply(BulbCommand::SetColour(Hsv::new(
                Hue::new(hue),
                Saturation::clamped_from_permille(saturation * 10),
                percent_brightness(brightness),
            )))
        }
        ["white", brightness, temperature] => {
            let brightness = number("brilho", brightness, 1, 100)?;
            let temperature = number("temperatura", temperature, 0, 100)?;
            apply(BulbCommand::SetWhite {
                brightness: percent_brightness(brightness),
                temperature: ColorTemperature::clamped_from_permille(temperature * 10),
            })
        }
        ["brightness", brightness] => {
            let brightness = number("brilho", brightness, 1, 100)?;
            apply(BulbCommand::SetBrightness(percent_brightness(brightness)))
        }
        [] => Err("faltou o comando do ctl".to_owned()),
        _ => Err(format!("comando do ctl inválido: {}", words.join(" "))),
    }
}

/// `x,y,largura,altura` em pixels físicos.
fn parse_anchor(text: &str) -> Option<Rect> {
    let numbers: Vec<i32> = text
        .split(',')
        .map(|part| part.trim().parse().ok())
        .collect::<Option<_>>()?;
    match numbers.as_slice() {
        [x, y, width, height] => Some(Rect {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        }),
        _ => None,
    }
}

/// Um número inteiro em `min..=max`.
fn number(name: &str, text: &str, min: u16, max: u16) -> Result<u16, String> {
    text.parse::<u16>()
        .ok()
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| format!("{name} inválido: \"{text}\" (use um número de {min} a {max})"))
}

/// `percent` já foi validado em 1..=100.
fn percent_brightness(percent: u16) -> Brightness {
    Brightness::clamped_from_permille(percent * 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_text(text: &str) -> Result<Mode, String> {
        let args: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        parse(&args)
    }

    fn ctl(text: &str) -> CtlCommand {
        match parse_text(text) {
            Ok(Mode::Ctl(command)) => command,
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn without_arguments_starts_the_daemon() {
        assert_eq!(parse(&[]), Ok(Mode::Daemon));
    }

    #[test]
    fn panel_and_help() {
        assert_eq!(parse_text("panel"), Ok(Mode::Panel(None)));
        assert_eq!(
            parse_text("panel --anchor 1500,-20,24,24"),
            Ok(Mode::Panel(Some(Rect {
                x: 1500,
                y: -20,
                width: 24,
                height: 24
            })))
        );
        assert_eq!(parse_text("panel --anchor lixo"), Ok(Mode::Panel(None)));
        assert_eq!(parse_text("panel --anchor 1,2,3"), Ok(Mode::Panel(None)));
        assert_eq!(parse_text("help"), Ok(Mode::Help));
        assert_eq!(parse_text("--help"), Ok(Mode::Help));
        assert_eq!(parse_text("-h"), Ok(Mode::Help));
    }

    #[test]
    fn simple_ctl_commands() {
        assert_eq!(ctl("ctl status"), CtlCommand::Status);
        assert_eq!(ctl("ctl discover"), CtlCommand::Discover);
        assert_eq!(ctl("ctl watch"), CtlCommand::Watch);
        assert_eq!(
            ctl("ctl login Abc1234"),
            CtlCommand::Login("Abc1234".to_owned())
        );
        assert_eq!(
            ctl("ctl on"),
            CtlCommand::Apply(BulbCommand::SetPower(Power::On))
        );
        assert_eq!(
            ctl("ctl off"),
            CtlCommand::Apply(BulbCommand::SetPower(Power::Off))
        );
        assert_eq!(
            ctl("ctl toggle"),
            CtlCommand::Apply(BulbCommand::TogglePower)
        );
    }

    #[test]
    fn color_takes_hue_degrees_and_percentages() {
        assert_eq!(
            ctl("ctl color 240 80 50"),
            CtlCommand::Apply(BulbCommand::SetColour(Hsv::new(
                Hue::new(240),
                Saturation::try_from_permille(800).unwrap(),
                Brightness::from_percent(50).unwrap(),
            )))
        );
    }

    #[test]
    fn white_takes_brightness_and_temperature_percentages() {
        assert_eq!(
            ctl("ctl white 30 100"),
            CtlCommand::Apply(BulbCommand::SetWhite {
                brightness: Brightness::from_percent(30).unwrap(),
                temperature: ColorTemperature::COOLEST,
            })
        );
    }

    #[test]
    fn brightness_takes_a_percentage() {
        assert_eq!(
            ctl("ctl brightness 75"),
            CtlCommand::Apply(BulbCommand::SetBrightness(
                Brightness::from_percent(75).unwrap()
            ))
        );
    }

    #[test]
    fn rejects_values_out_of_range_or_not_numbers() {
        for text in [
            "ctl color 360 50 50",
            "ctl color 10 101 50",
            "ctl color 10 50 0",
            "ctl white 0 50",
            "ctl white 50 101",
            "ctl brightness 0",
            "ctl brightness muito",
            "ctl brightness -5",
        ] {
            assert!(parse_text(text).is_err(), "{text} deveria falhar");
        }
    }

    #[test]
    fn rejects_missing_or_extra_arguments() {
        for text in [
            "ctl",
            "ctl color 10 20",
            "ctl white 10",
            "ctl brightness",
            "ctl status agora",
            "ctl login",
            "ctl dance",
            "tray",
        ] {
            assert!(parse_text(text).is_err(), "{text} deveria falhar");
        }
    }

    #[test]
    fn error_messages_explain_the_problem() {
        let error = parse_text("ctl brightness 0").unwrap_err();

        assert!(error.contains("brilho"), "{error}");
        assert!(error.contains("1 a 100"), "{error}");
    }
}
