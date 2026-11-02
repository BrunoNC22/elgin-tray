//! Atalhos globais com `RegisterHotKey`, numa thread própria com loop de mensagens (o Windows
//! entrega o `WM_HOTKEY` na fila da thread que registrou).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use elgin_application::hotkeys::{HotkeyAction, HotkeyBinding, Key, Modifiers};
use elgin_application::ports::HotkeyRegistrar;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey, VK_DELETE, VK_DOWN, VK_END, VK_F1, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT,
    VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY, WM_USER,
};

const WM_REGISTER: u32 = WM_APP + 1;
const WM_STOP: u32 = WM_APP + 2;
/// Quanto esperar a thread dos atalhos responder a um registro.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);
/// `ERROR_HOTKEY_ALREADY_REGISTERED`.
const ALREADY_REGISTERED: i32 = 1409;

type Bindings = Vec<(HotkeyAction, HotkeyBinding)>;
type Failures = Vec<(HotkeyAction, String)>;
type Pending = Arc<Mutex<Option<(Bindings, mpsc::Sender<Failures>)>>>;

/// Os atalhos globais do Windows. Cada atalho apertado chama o `on_hotkey` dado em [`start`].
///
/// [`start`]: WindowsHotkeys::start
#[derive(Debug)]
pub struct WindowsHotkeys {
    thread_id: u32,
    pending: Pending,
}

impl WindowsHotkeys {
    /// Sobe a thread dos atalhos (ainda sem nenhum registrado).
    pub fn start<F>(on_hotkey: F) -> Result<Self, String>
    where
        F: Fn(HotkeyAction) + Send + 'static,
    {
        let pending: Pending = Arc::default();
        let thread_pending = Arc::clone(&pending);
        let (ready, thread_id) = mpsc::channel();
        std::thread::Builder::new()
            .name("atalhos".to_owned())
            .spawn(move || message_loop(&thread_pending, &on_hotkey, &ready))
            .map_err(|error| format!("não foi possível criar a thread dos atalhos: {error}"))?;
        let thread_id = thread_id
            .recv()
            .map_err(|_| "a thread dos atalhos não iniciou".to_owned())?;
        Ok(Self { thread_id, pending })
    }

    fn post(&self, message: u32) -> bool {
        // SAFETY: só posta uma mensagem sem ponteiros para a fila de uma thread.
        #[allow(unsafe_code)]
        unsafe {
            PostThreadMessageW(self.thread_id, message, 0, 0) != 0
        }
    }
}

impl HotkeyRegistrar for WindowsHotkeys {
    fn register(&mut self, bindings: &[(HotkeyAction, HotkeyBinding)]) -> Failures {
        let (reply, replied) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some((bindings.to_vec(), reply));
        }
        let unanswered = |reason: &str| -> Failures {
            bindings
                .iter()
                .map(|&(action, _)| (action, reason.to_owned()))
                .collect()
        };
        if !self.post(WM_REGISTER) {
            return unanswered("a thread dos atalhos parou");
        }
        replied
            .recv_timeout(REPLY_TIMEOUT)
            .unwrap_or_else(|_| unanswered("a thread dos atalhos não respondeu"))
    }
}

impl Drop for WindowsHotkeys {
    fn drop(&mut self) {
        self.post(WM_STOP);
    }
}

/// O loop da thread dos atalhos: registra o que chega em `pending` e repassa os `WM_HOTKEY`.
fn message_loop(pending: &Pending, on_hotkey: &dyn Fn(HotkeyAction), ready: &mpsc::Sender<u32>) {
    // O id de cada atalho registrado é a posição nesta lista + 1.
    let mut registered: Vec<HotkeyAction> = Vec::new();
    // SAFETY: chamadas Win32 sem ponteiros guardados; `msg` é local. O PeekMessage cria a fila da
    // thread antes de alguém postar nela.
    #[allow(unsafe_code)]
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(
            &mut msg,
            std::ptr::null_mut(),
            WM_USER,
            WM_USER,
            PM_NOREMOVE,
        );
        let _ = ready.send(windows_sys::Win32::System::Threading::GetCurrentThreadId());
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            match msg.message {
                WM_HOTKEY => {
                    let action = msg.wParam.checked_sub(1).and_then(|i| registered.get(i));
                    if let Some(&action) = action {
                        on_hotkey(action);
                    }
                }
                WM_REGISTER => {
                    let request = pending.lock().ok().and_then(|mut pending| pending.take());
                    if let Some((bindings, reply)) = request {
                        let _ = reply.send(replace_all(&mut registered, &bindings));
                    }
                }
                WM_STOP => break,
                _ => {}
            }
        }
        unregister_all(&mut registered);
    }
}

/// Tira os atalhos antigos e registra os novos. Devolve os que falharam. Chame na thread dos
/// atalhos.
fn replace_all(registered: &mut Vec<HotkeyAction>, bindings: &Bindings) -> Failures {
    unregister_all(registered);
    let mut failures = Vec::new();
    for &(action, binding) in bindings {
        registered.push(action);
        let id = registered.len() as i32;
        // SAFETY: registro sem janela (`hwnd` nulo): o WM_HOTKEY vai para a fila desta thread.
        #[allow(unsafe_code)]
        let ok = unsafe {
            RegisterHotKey(
                std::ptr::null_mut(),
                id,
                modifier_flags(binding.modifiers),
                virtual_key(binding.key),
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            let reason = if error.raw_os_error() == Some(ALREADY_REGISTERED) {
                "já usado por outro programa".to_owned()
            } else {
                error.to_string()
            };
            failures.push((action, reason));
        }
    }
    failures
}

/// Desregistra todos os atalhos desta thread.
fn unregister_all(registered: &mut Vec<HotkeyAction>) {
    for id in 1..=registered.len() {
        // SAFETY: desregistrar um id que não pegou só falha, sem efeito.
        #[allow(unsafe_code)]
        unsafe {
            UnregisterHotKey(std::ptr::null_mut(), id as i32);
        }
    }
    registered.clear();
}

/// As modificadoras no formato do `RegisterHotKey` (sem repetir enquanto a tecla fica apertada).
fn modifier_flags(modifiers: Modifiers) -> HOT_KEY_MODIFIERS {
    [
        (modifiers.ctrl, MOD_CONTROL),
        (modifiers.alt, MOD_ALT),
        (modifiers.shift, MOD_SHIFT),
        (modifiers.super_key, MOD_WIN),
    ]
    .into_iter()
    .filter(|&(on, _)| on)
    .fold(MOD_NOREPEAT, |flags, (_, flag)| flags | flag)
}

/// O código de tecla virtual do Windows.
fn virtual_key(key: Key) -> u32 {
    let code = match key {
        // Letras maiúsculas e dígitos têm o próprio código ASCII.
        Key::Letter(letter) => letter as u16,
        Key::Digit(digit) => u16::from(b'0' + digit),
        Key::Function(number) => VK_F1 + u16::from(number) - 1,
        Key::Up => VK_UP,
        Key::Down => VK_DOWN,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
        Key::Space => VK_SPACE,
        Key::Enter => VK_RETURN,
        Key::Tab => VK_TAB,
        Key::Home => VK_HOME,
        Key::End => VK_END,
        Key::PageUp => VK_PRIOR,
        Key::PageDown => VK_NEXT,
        Key::Insert => VK_INSERT,
        Key::Delete => VK_DELETE,
    };
    u32::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(text: &str) -> HotkeyBinding {
        text.parse().unwrap()
    }

    #[test]
    fn maps_modifiers_without_repeat() {
        assert_eq!(
            modifier_flags(binding("Ctrl+Alt+L").modifiers),
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT
        );
        assert_eq!(
            modifier_flags(binding("Shift+Win+L").modifiers),
            MOD_SHIFT | MOD_WIN | MOD_NOREPEAT
        );
    }

    #[test]
    fn maps_keys_to_virtual_key_codes() {
        let cases = [
            ("Ctrl+L", 0x4C),
            ("Ctrl+7", 0x37),
            ("Ctrl+F1", 0x70),
            ("Ctrl+F24", 0x87),
            ("Ctrl+Up", 0x26),
            ("Ctrl+Down", 0x28),
            ("Ctrl+Left", 0x25),
            ("Ctrl+Right", 0x27),
            ("Ctrl+PageUp", 0x21),
            ("Ctrl+PageDown", 0x22),
            ("Ctrl+Delete", 0x2E),
        ];
        for (text, code) in cases {
            assert_eq!(virtual_key(binding(text).key), code, "{text}");
        }
    }
}
