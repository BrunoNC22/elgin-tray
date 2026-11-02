//! Monitor e área de trabalho no Windows (onde a barra de tarefas não está).

use crate::placement::Rect;
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// (monitor, área de trabalho) do monitor que contém `point` (ou o do cursor, sem ponto).
pub(crate) fn monitor_at(point: Option<(i32, i32)>) -> Option<(Rect, Rect)> {
    // SAFETY: chamadas Win32 com estruturas locais inicializadas e `cbSize` preenchido.
    #[allow(unsafe_code)]
    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        let point = match point {
            Some((x, y)) => POINT { x, y },
            None => {
                GetCursorPos(&mut cursor);
                cursor
            }
        };
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some((rect(info.rcMonitor), rect(info.rcWork)))
    }
}

fn rect(r: RECT) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        width: r.right - r.left,
        height: r.bottom - r.top,
    }
}
