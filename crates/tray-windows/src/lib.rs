//! Adaptador: bandeja do Windows.
//!
//! Só compila no Windows; nos outros sistemas a crate fica vazia.
//!
//! O ícone de notificação do Windows precisa de um loop de mensagens na thread que o criou. Por isso
//! [`run`] fica preso na thread principal: o daemon (em outra thread) guarda o último
//! [`TrayViewModel`] no [`SharedView`] e acorda o loop com [`TrayWaker::refresh`]; os cliques e itens
//! do menu voltam pelo callback `on_event`. As notificações saem como balão do próprio ícone
//! ([`TrayNotifier`]; no Windows 10/11 elas aparecem como notificação comum).

#![cfg(windows)]

use std::sync::{Arc, Mutex};

use elgin_application::ports::Notifier;
use elgin_application::tray::{IconSpec, MenuAction, MenuItem, TrayAction, TrayViewModel};
use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem as MudaItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW,
    TranslateMessage, WM_APP, WM_USER,
};

/// O último estado da bandeja, compartilhado com a thread do daemon.
pub type SharedView = Arc<Mutex<Option<TrayViewModel>>>;

/// Onde o ícone está na tela, em pixels físicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

const WM_REFRESH: u32 = WM_APP + 1;
const WM_QUIT_TRAY: u32 = WM_APP + 2;
/// Tamanho do bitmap entregue ao Windows (ele reduz para o tamanho da bandeja).
const ICON_SIZE: u32 = 32;

/// Acorda o loop da bandeja a partir de outra thread.
#[derive(Debug, Clone, Copy)]
pub struct TrayWaker {
    thread_id: u32,
}

impl TrayWaker {
    /// Há um [`TrayViewModel`] novo no [`SharedView`].
    pub fn refresh(&self) {
        self.post(WM_REFRESH);
    }

    /// Remove o ícone e termina o loop.
    pub fn quit(&self) {
        self.post(WM_QUIT_TRAY);
    }

    fn post(&self, message: u32) {
        // SAFETY: só posta uma mensagem sem ponteiros para a fila de uma thread.
        #[allow(unsafe_code)]
        unsafe {
            PostThreadMessageW(self.thread_id, message, 0, 0);
        }
    }
}

/// Notificações em balão saindo do ícone da bandeja. Pode ser usado de qualquer thread.
#[derive(Debug)]
pub struct TrayNotifier {
    /// A janela escondida que o `tray-icon` criou para o ícone (guardada como número para poder
    /// mudar de thread).
    window: isize,
    /// O id que o `tray-icon` deu ao ícone (interno; descoberto na primeira notificação).
    icon_id: Option<u32>,
}

/// Até onde procurar o id do ícone (o `tray-icon` numera a partir de 1, e temos só um ícone).
const MAX_ICON_ID: u32 = 16;

impl TrayNotifier {
    /// Manda o balão para o ícone `id`. Falha se o id não é o do ícone.
    fn show(&self, id: u32, title: &str, body: &str) -> bool {
        use windows_sys::Win32::UI::Shell::{
            NIF_INFO, NIIF_RESPECT_QUIET_TIME, NIIF_USER, NIM_MODIFY, NOTIFYICONDATAW,
            Shell_NotifyIconW,
        };

        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window as _,
            uID: id,
            uFlags: NIF_INFO,
            // Com NIIF_USER o balão usa o próprio ícone da lâmpada.
            dwInfoFlags: NIIF_USER | NIIF_RESPECT_QUIET_TIME,
            ..Default::default()
        };
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, body);
        // SAFETY: `data` é uma estrutura local completa; o Windows só lê dela durante a chamada.
        #[allow(unsafe_code)]
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &data) != 0
        }
    }
}

impl Notifier for TrayNotifier {
    fn notify(&mut self, title: &str, body: &str) {
        if let Some(id) = self.icon_id
            && self.show(id, title, body)
        {
            return;
        }
        // O id é interno do `tray-icon`: tenta os primeiros até o Windows aceitar.
        self.icon_id = (1..=MAX_ICON_ID).find(|&id| self.show(id, title, body));
        if self.icon_id.is_none() {
            log::warn!("não foi possível mostrar a notificação \"{title}\"");
        }
    }
}

/// Copia `text` para um buffer UTF-16 de tamanho fixo, cortando se precisar e terminando em zero.
fn copy_wide<const N: usize>(buffer: &mut [u16; N], text: &str) {
    let mut units = text.encode_utf16().take(N - 1).collect::<Vec<_>>();
    // Não deixa meio par substituto no fim de um texto cortado.
    if units
        .last()
        .is_some_and(|&unit| (0xD800..0xDC00).contains(&unit))
    {
        units.pop();
    }
    buffer[..units.len()].copy_from_slice(&units);
    buffer[units.len()] = 0;
}

/// Mostra o ícone e roda o loop de mensagens até [`TrayWaker::quit`]. Chame na thread principal.
///
/// `on_ready` recebe o [`TrayWaker`] e o [`TrayNotifier`] assim que o loop pode receber mensagens.
pub fn run<E, R>(shared: SharedView, on_event: E, on_ready: R) -> Result<(), String>
where
    E: Fn(TrayAction, Option<IconRect>) + Send + Sync + 'static,
    R: FnOnce(TrayWaker, TrayNotifier),
{
    // SAFETY: chamadas Win32 sem ponteiros guardados; `msg` vive até o fim da função. O PeekMessage
    // só cria a fila de mensagens da thread antes de alguém postar nela.
    #[allow(unsafe_code)]
    let thread_id = unsafe {
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(
            &mut msg,
            std::ptr::null_mut(),
            WM_USER,
            WM_USER,
            PM_NOREMOVE,
        );
        GetCurrentThreadId()
    };

    let on_event = Arc::new(on_event);
    let clicks = Arc::clone(&on_event);
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button,
            button_state: MouseButtonState::Up,
            rect,
            ..
        } = event
        {
            let anchor = IconRect {
                x: rect.position.x as i32,
                y: rect.position.y as i32,
                width: rect.size.width,
                height: rect.size.height,
            };
            match button {
                MouseButton::Left => clicks(TrayAction::Click, Some(anchor)),
                MouseButton::Middle => clicks(TrayAction::MiddleClick, Some(anchor)),
                MouseButton::Right => {}
            }
        }
    }));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(action) = MenuAction::from_id(event.id.as_ref()) {
            on_event(TrayAction::Menu(action), None);
        }
    }));

    let tray = TrayIconBuilder::new()
        .with_icon(icon(&IconSpec::Unavailable(
            elgin_application::tray::Indicator::Warning,
        ))?)
        .with_tooltip("elgin-tray")
        .with_menu_on_left_click(false)
        .build()
        .map_err(|error| format!("não foi possível criar o ícone da bandeja: {error}"))?;

    on_ready(
        TrayWaker { thread_id },
        TrayNotifier {
            window: tray.window_handle() as isize,
            icon_id: None,
        },
    );
    apply(&tray, &shared);

    // SAFETY: loop de mensagens Win32 padrão; `msg` é uma variável local válida.
    #[allow(unsafe_code)]
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_REFRESH {
                apply(&tray, &shared);
            } else if msg.hwnd.is_null() && msg.message == WM_QUIT_TRAY {
                break;
            } else {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    drop(tray);
    TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
    MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    Ok(())
}

/// Mostra no ícone o último estado compartilhado.
fn apply(tray: &TrayIcon, shared: &SharedView) {
    let Some(view) = shared.lock().ok().and_then(|view| view.clone()) else {
        return;
    };
    if let Ok(icon) = icon(&view.icon) {
        let _ = tray.set_icon(Some(icon));
    }
    let _ = tray.set_tooltip(Some(&view.tooltip));
    tray.set_menu(Some(Box::new(menu(&view.menu))));
}

fn icon(spec: &IconSpec) -> Result<Icon, String> {
    let image = elgin_icons::render(spec, ICON_SIZE);
    Icon::from_rgba(image.data, image.width, image.height).map_err(|error| error.to_string())
}

fn menu(items: &[MenuItem]) -> Menu {
    let menu = Menu::new();
    for item in items {
        append(&menu, item);
    }
    menu
}

/// Acrescenta um item (e os filhos, se for submenu) ao menu do `muda`.
fn append(parent: &dyn AppendItem, item: &MenuItem) {
    match item {
        MenuItem::Action {
            action,
            label,
            enabled,
        } => parent.append_item(&MudaItem::with_id(
            MenuId::new(action.id()),
            label,
            *enabled,
            None,
        )),
        MenuItem::Check {
            action,
            label,
            checked,
            enabled,
        } => parent.append_item(&CheckMenuItem::with_id(
            MenuId::new(action.id()),
            label,
            *enabled,
            *checked,
            None,
        )),
        MenuItem::Submenu {
            label,
            enabled,
            items,
        } => {
            let submenu = Submenu::new(label, *enabled);
            for child in items {
                append(&submenu, child);
            }
            parent.append_item(&submenu);
        }
        MenuItem::Separator => parent.append_item(&PredefinedMenuItem::separator()),
    }
}

/// Menu e submenu do `muda` têm `append` separados; este trait une os dois.
trait AppendItem {
    fn append_item(&self, item: &dyn tray_icon::menu::IsMenuItem);
}

impl AppendItem for Menu {
    fn append_item(&self, item: &dyn tray_icon::menu::IsMenuItem) {
        let _ = self.append(item);
    }
}

impl AppendItem for Submenu {
    fn append_item(&self, item: &dyn tray_icon::menu::IsMenuItem) {
        let _ = self.append(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_text_is_cut_and_terminated() {
        let mut buffer = [0xFFFF_u16; 6];
        copy_wide(&mut buffer, "lâmpada");
        assert_eq!(String::from_utf16(&buffer[..5]).unwrap(), "lâmpa");
        assert_eq!(buffer[5], 0);

        let mut buffer = [0xFFFF_u16; 6];
        copy_wide(&mut buffer, "oi");
        assert_eq!(&buffer[..3], &[u16::from(b'o'), u16::from(b'i'), 0]);
    }

    #[test]
    fn a_cut_never_splits_a_surrogate_pair() {
        let mut buffer = [0xFFFF_u16; 4];
        copy_wide(&mut buffer, "ab💡");
        assert_eq!(&buffer[..3], &[u16::from(b'a'), u16::from(b'b'), 0]);
    }
}
