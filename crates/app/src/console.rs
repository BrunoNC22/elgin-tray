//! Console no Windows.
//!
//! Quando o executável virar de subsistema GUI (#21), ele não terá console próprio: no modo `ctl`
//! ele se liga ao console do terminal que o chamou, para conseguir imprimir. Enquanto o executável
//! for de console, a chamada só falha (já há um console) e nada muda.

/// Liga a saída ao console do processo pai, se houver.
#[cfg(windows)]
pub(crate) fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole não recebe ponteiros; só falha (retorna 0) se já há console ou o pai
    // não tem um, e as duas situações podem ser ignoradas.
    #[allow(unsafe_code)]
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub(crate) fn attach_parent_console() {}
