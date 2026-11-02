//! Iniciar com o sistema: um valor em `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! apontando para o executável (não precisa de administrador).

use std::io;
use std::path::PathBuf;

use elgin_application::ports::AutostartRegistrar;

/// Nome do valor no `Run`.
const VALUE_NAME: &str = "elgin-tray";

/// A chave `Run` do usuário (atrás de uma trait para os testes não mexerem no registro).
pub trait RunKey {
    /// Cria ou troca o valor `name` com o comando `command`.
    fn set(&mut self, name: &str, command: &str) -> io::Result<()>;
    /// Apaga o valor `name`; não existir não é erro.
    fn remove(&mut self, name: &str) -> io::Result<()>;
}

/// Registra o executável no `Run` do usuário.
#[derive(Debug)]
pub struct WindowsAutostart<K> {
    key: K,
    executable: PathBuf,
}

impl<K: RunKey> WindowsAutostart<K> {
    pub fn new(key: K, executable: PathBuf) -> Self {
        Self { key, executable }
    }
}

impl WindowsAutostart<HkcuRunKey> {
    /// Com o registro de verdade e o executável que está rodando.
    pub fn for_current_exe() -> io::Result<Self> {
        Ok(Self::new(HkcuRunKey, std::env::current_exe()?))
    }
}

impl<K: RunKey> AutostartRegistrar for WindowsAutostart<K> {
    fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let result = if enabled {
            // Entre aspas: o caminho pode ter espaços (ex.: o nome do usuário).
            let command = format!("\"{}\"", self.executable.display());
            self.key.set(VALUE_NAME, &command)
        } else {
            self.key.remove(VALUE_NAME)
        };
        result.map_err(|error| format!("registro do Windows: {error}"))
    }
}

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` de verdade.
#[derive(Debug, Clone, Copy, Default)]
pub struct HkcuRunKey;

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

impl RunKey for HkcuRunKey {
    #[allow(unsafe_code)]
    fn set(&mut self, name: &str, command: &str) -> io::Result<()> {
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};

        let subkey = wide(RUN_SUBKEY);
        let name = wide(name);
        let data = wide(command);
        let bytes = u32::try_from(data.len() * 2).map_err(io::Error::other)?;
        // SAFETY: as strings terminam em zero e vivem até o fim da chamada; `bytes` é o tamanho
        // de `data` em bytes, incluindo o zero final, como o REG_SZ pede.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                bytes,
            )
        };
        check(status)
    }

    #[allow(unsafe_code)]
    fn remove(&mut self, name: &str) -> io::Result<()> {
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyValueW};

        let subkey = wide(RUN_SUBKEY);
        let name = wide(name);
        // SAFETY: as strings terminam em zero e vivem até o fim da chamada.
        let status =
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        check(status)
    }
}

fn check(status: u32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Debug, Default)]
    struct FakeRunKey {
        values: HashMap<String, String>,
        broken: bool,
    }

    impl RunKey for FakeRunKey {
        fn set(&mut self, name: &str, command: &str) -> io::Result<()> {
            if self.broken {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            self.values.insert(name.to_owned(), command.to_owned());
            Ok(())
        }

        fn remove(&mut self, name: &str) -> io::Result<()> {
            self.values.remove(name);
            Ok(())
        }
    }

    fn autostart() -> WindowsAutostart<FakeRunKey> {
        WindowsAutostart::new(
            FakeRunKey::default(),
            PathBuf::from(r"C:\Users\Ana Maria\AppData\Local\elgin-tray\elgin-tray.exe"),
        )
    }

    #[test]
    fn enabling_points_the_run_value_at_the_quoted_executable() {
        let mut autostart = autostart();

        autostart.set_enabled(true).unwrap();

        assert_eq!(
            autostart.key.values[VALUE_NAME],
            r#""C:\Users\Ana Maria\AppData\Local\elgin-tray\elgin-tray.exe""#
        );
    }

    #[test]
    fn disabling_removes_the_value_even_when_missing() {
        let mut autostart = autostart();
        autostart.set_enabled(true).unwrap();

        autostart.set_enabled(false).unwrap();
        assert!(autostart.key.values.is_empty());
        autostart.set_enabled(false).unwrap();
    }

    #[test]
    fn registry_errors_are_reported() {
        let mut autostart = autostart();
        autostart.key.broken = true;

        assert!(autostart.set_enabled(true).is_err());
    }
}
