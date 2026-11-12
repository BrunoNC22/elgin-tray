//! Atalhos globais no KDE pelo KGlobalAccel (Wayland e X11): os atalhos aparecem em Configurações
//! do Sistema → Atalhos, no componente "Elgin Tray". A fonte das teclas continua sendo o
//! `config.toml`: a cada início elas são gravadas de novo (sem o carregamento automático do KDE).
//!
//! O `zbus` bloqueante cria outro runtime do tokio, o que não pode acontecer dentro do runtime do
//! daemon: como no Windows, os atalhos têm uma thread própria (com o seu runtime), que registra o
//! que chega por [`HotkeyRegistrar::register`] e repassa os atalhos apertados ao callback.

use std::sync::mpsc;
use std::time::Duration;

use elgin_application::hotkeys::{HotkeyAction, HotkeyBinding, Key};
use elgin_application::ports::HotkeyRegistrar;

/// Nome do componente no KGlobalAccel.
const COMPONENT: &str = "elgin-tray";
/// Nome do componente nas Configurações do Sistema.
const COMPONENT_LABEL: &str = "Elgin Tray";
/// Quanto esperar a thread dos atalhos responder.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

// Flags do `setShortcut` (KGlobalAccelD::SetShortcutFlag).
const IS_DEFAULT: u32 = 1;
const SET_PRESENT: u32 = 2;
const NO_AUTOLOADING: u32 = 4;

// Modificadoras e teclas no código do Qt (`Qt::KeyboardModifier` | `Qt::Key`).
const QT_SHIFT: i32 = 0x0200_0000;
const QT_CTRL: i32 = 0x0400_0000;
const QT_ALT: i32 = 0x0800_0000;
const QT_META: i32 = 0x1000_0000;

const SERVICE: &str = "org.kde.kglobalaccel";
const INTERFACE: &str = "org.kde.KGlobalAccel";

type Bindings = Vec<(HotkeyAction, HotkeyBinding)>;
type Failures = Vec<(HotkeyAction, String)>;
/// Atalhos a registrar e por onde responder quais falharam.
type Request = (Bindings, mpsc::Sender<Failures>);

/// Os atalhos globais do KDE. Ao ser descartado, a thread termina e o KDE desativa os atalhos.
#[derive(Debug)]
pub struct KdeHotkeys {
    requests: tokio::sync::mpsc::UnboundedSender<Request>,
}

impl KdeHotkeys {
    /// Sobe a thread dos atalhos (ainda sem nenhum registrado). `on_hotkey` é chamado nela.
    pub fn start<F>(on_hotkey: F) -> Result<Self, String>
    where
        F: Fn(HotkeyAction) + Send + 'static,
    {
        let (requests, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (ready, started) = mpsc::channel();
        std::thread::Builder::new()
            .name("atalhos".to_owned())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(serve(receiver, on_hotkey, ready)),
                    Err(error) => {
                        let _ = ready.send(Err(error.to_string()));
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        started
            .recv_timeout(REPLY_TIMEOUT)
            .map_err(|_| "a thread dos atalhos não respondeu".to_owned())??;
        Ok(Self { requests })
    }
}

impl HotkeyRegistrar for KdeHotkeys {
    fn register(&mut self, bindings: &[(HotkeyAction, HotkeyBinding)]) -> Failures {
        let unanswered = |reason: &str| -> Failures {
            bindings
                .iter()
                .map(|&(action, _)| (action, reason.to_owned()))
                .collect()
        };
        let (reply, replied) = mpsc::channel();
        if self.requests.send((bindings.to_vec(), reply)).is_err() {
            return unanswered("a thread dos atalhos parou");
        }
        replied
            .recv_timeout(REPLY_TIMEOUT)
            .unwrap_or_else(|_| unanswered("o KDE não respondeu"))
    }
}

/// O loop da thread: registra o que chega e repassa os atalhos apertados.
async fn serve<F>(
    mut requests: tokio::sync::mpsc::UnboundedReceiver<Request>,
    on_hotkey: F,
    ready: mpsc::Sender<Result<(), String>>,
) where
    F: Fn(HotkeyAction),
{
    use futures_util::StreamExt;

    let path = component_path(COMPONENT);
    let connected = async {
        let connection = zbus::Connection::session().await?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(SERVICE)?
            .path(path.as_str())?
            .interface("org.kde.kglobalaccel.Component")?
            .member("globalShortcutPressed")?
            .build();
        let pressed = zbus::MessageStream::for_match_rule(rule, &connection, None).await?;
        Ok::<_, zbus::Error>((connection, pressed))
    }
    .await;
    let (connection, mut pressed) = match connected {
        Ok(connected) => {
            let _ = ready.send(Ok(()));
            connected
        }
        Err(error) => {
            let _ = ready.send(Err(format!("sem acesso ao KDE pelo DBus: {error}")));
            return;
        }
    };
    loop {
        tokio::select! {
            request = requests.recv() => match request {
                Some((bindings, reply)) => {
                    let _ = reply.send(apply(&connection, &bindings).await);
                }
                None => return,
            },
            Some(Ok(message)) = pressed.next() => {
                let body = message.body();
                if let Ok((component, name, _timestamp)) = body.deserialize::<(String, String, i64)>()
                    && component == COMPONENT
                    && let Some(action) = action_named(&name)
                {
                    on_hotkey(action);
                }
            }
        }
    }
}

/// Troca os atalhos do app pelos de `bindings`. Primeiro tira todos (assim trocar as teclas entre
/// duas ações não parece conflito consigo mesmo), depois grava cada um como padrão e como atual,
/// sem o carregamento automático do KDE (a fonte é o `config.toml`).
async fn apply(
    connection: &zbus::Connection,
    bindings: &[(HotkeyAction, HotkeyBinding)],
) -> Failures {
    for action in HotkeyAction::ALL {
        let _ = call(connection, "unregister", &(COMPONENT, action_name(action))).await;
    }
    let mut failures = Vec::new();
    for &(action, binding) in bindings {
        let key = qt_key(&binding);
        // O KGlobalAccel aceita gravar uma tecla que já é de outro componente (e aí quem recebe é
        // o outro): é preciso perguntar antes.
        match is_available(connection, key).await {
            Ok(true) => {}
            Ok(false) => {
                failures.push((action, "já usado por outro programa".to_owned()));
                continue;
            }
            Err(error) => {
                failures.push((action, format!("o KDE recusou ({error})")));
                continue;
            }
        }
        match set_shortcut(connection, action, key).await {
            Ok(applied) if applied.contains(&key) => {}
            Ok(_) => failures.push((action, "já usado por outro programa".to_owned())),
            Err(error) => failures.push((action, format!("o KDE recusou ({error})"))),
        }
    }
    failures
}

/// `key` não é de nenhum outro componente.
async fn is_available(connection: &zbus::Connection, key: i32) -> zbus::Result<bool> {
    call(connection, "isGlobalShortcutAvailable", &(key, COMPONENT))
        .await?
        .body()
        .deserialize()
}

/// Registra a ação e grava `key`; devolve as teclas que o KDE aceitou.
async fn set_shortcut(
    connection: &zbus::Connection,
    action: HotkeyAction,
    key: i32,
) -> zbus::Result<Vec<i32>> {
    let id = action_id(action);
    call(connection, "doRegister", &(&id,)).await?;
    call(connection, "setShortcut", &(&id, vec![key], IS_DEFAULT)).await?;
    call(
        connection,
        "setShortcut",
        &(&id, vec![key], SET_PRESENT | NO_AUTOLOADING),
    )
    .await?
    .body()
    .deserialize()
}

async fn call<B>(
    connection: &zbus::Connection,
    method: &str,
    body: &B,
) -> zbus::Result<zbus::Message>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    connection
        .call_method(
            Some(SERVICE),
            "/kglobalaccel",
            Some(INTERFACE),
            method,
            body,
        )
        .await
}

/// O atalho como o KGlobalAccel recebe (`QKeyCombination` em um inteiro).
fn qt_key(binding: &HotkeyBinding) -> i32 {
    let key = match binding.key {
        Key::Letter(letter) => letter as i32,
        Key::Digit(digit) => i32::from(b'0' + digit),
        Key::Function(number) => 0x0100_0030 + i32::from(number) - 1,
        Key::Up => 0x0100_0013,
        Key::Down => 0x0100_0015,
        Key::Left => 0x0100_0012,
        Key::Right => 0x0100_0014,
        Key::Space => 0x20,
        Key::Enter => 0x0100_0004,
        Key::Tab => 0x0100_0001,
        Key::Home => 0x0100_0010,
        Key::End => 0x0100_0011,
        Key::PageUp => 0x0100_0016,
        Key::PageDown => 0x0100_0017,
        Key::Insert => 0x0100_0006,
        Key::Delete => 0x0100_0007,
    };
    let modifiers = binding.modifiers;
    [
        (modifiers.shift, QT_SHIFT),
        (modifiers.ctrl, QT_CTRL),
        (modifiers.alt, QT_ALT),
        (modifiers.super_key, QT_META),
    ]
    .into_iter()
    .filter(|&(on, _)| on)
    .fold(key, |code, (_, flag)| code | flag)
}

/// Nome estável da ação no KGlobalAccel.
fn action_name(action: HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::Toggle => "toggle",
        HotkeyAction::BrightnessUp => "brightness_up",
        HotkeyAction::BrightnessDown => "brightness_down",
        HotkeyAction::NextFavorite => "next_favorite",
    }
}

fn action_named(name: &str) -> Option<HotkeyAction> {
    HotkeyAction::ALL
        .into_iter()
        .find(|&action| action_name(action) == name)
}

/// `[componente, ação, nome do componente, nome da ação]`, como o KGlobalAccel identifica a ação.
fn action_id(action: HotkeyAction) -> Vec<String> {
    let mut label = action.label().to_owned();
    if let Some(first) = label.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    vec![
        COMPONENT.to_owned(),
        action_name(action).to_owned(),
        COMPONENT_LABEL.to_owned(),
        label,
    ]
}

/// Caminho do objeto do componente (o KGlobalAccel troca o que não é letra, número ou `_` por `_`).
fn component_path(component: &str) -> String {
    let name: String = component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("/component/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use elgin_application::hotkeys::Modifiers;

    fn binding(text: &str) -> HotkeyBinding {
        text.parse().unwrap()
    }

    #[test]
    fn keys_use_the_qt_codes() {
        assert_eq!(qt_key(&binding("Ctrl+Alt+L")), QT_CTRL | QT_ALT | 0x4C);
        assert_eq!(
            qt_key(&binding("Ctrl+Alt+Up")),
            QT_CTRL | QT_ALT | 0x0100_0013
        );
        assert_eq!(qt_key(&binding("Shift+Win+5")), QT_SHIFT | QT_META | 0x35);
        assert_eq!(qt_key(&binding("Alt+F12")), QT_ALT | 0x0100_003B);
        assert_eq!(qt_key(&binding("Ctrl+Space")), QT_CTRL | 0x20);
        assert_eq!(qt_key(&binding("Ctrl+Enter")), QT_CTRL | 0x0100_0004);
        assert_eq!(qt_key(&binding("Ctrl+Delete")), QT_CTRL | 0x0100_0007);
    }

    #[test]
    fn every_key_has_a_code() {
        let keys = [
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Tab,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Insert,
        ];
        let codes: Vec<i32> = keys
            .iter()
            .map(|&key| {
                qt_key(&HotkeyBinding {
                    modifiers: Modifiers::default(),
                    key,
                })
            })
            .collect();
        assert_eq!(
            codes,
            [
                0x0100_0013,
                0x0100_0015,
                0x0100_0012,
                0x0100_0014,
                0x0100_0001,
                0x0100_0010,
                0x0100_0011,
                0x0100_0016,
                0x0100_0017,
                0x0100_0006
            ]
        );
    }

    #[test]
    fn actions_have_stable_names_and_labels() {
        for action in HotkeyAction::ALL {
            assert_eq!(action_named(action_name(action)), Some(action));
        }
        assert_eq!(action_named("outra"), None);
        assert_eq!(
            action_id(HotkeyAction::Toggle),
            ["elgin-tray", "toggle", "Elgin Tray", "Ligar/desligar"]
        );
    }

    #[test]
    fn the_component_path_replaces_unsafe_characters() {
        assert_eq!(component_path("elgin-tray"), "/component/elgin_tray");
        assert_eq!(component_path("a.b c_1"), "/component/a_b_c_1");
    }
}
