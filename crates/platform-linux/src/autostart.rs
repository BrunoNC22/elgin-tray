//! Iniciar com o sistema: `~/.config/autostart/elgin-tray.desktop` (padrão XDG, lido pelo KDE).

use std::io;
use std::path::{Path, PathBuf};

use elgin_application::ports::AutostartRegistrar;

const FILE_NAME: &str = "elgin-tray.desktop";

/// O sistema de arquivos visto pelo autostart (atrás de uma trait para os testes).
pub trait AutostartFiles {
    /// Grava o arquivo, criando a pasta se preciso.
    fn write(&mut self, path: &Path, contents: &str) -> io::Result<()>;
    /// Apaga o arquivo; não existir não é erro.
    fn remove(&mut self, path: &Path) -> io::Result<()>;
}

/// Cria ou apaga o `.desktop` de autostart.
#[derive(Debug)]
pub struct DesktopAutostart<F> {
    files: F,
    /// A pasta `autostart` (normalmente `~/.config/autostart`).
    directory: PathBuf,
    executable: PathBuf,
}

impl<F: AutostartFiles> DesktopAutostart<F> {
    pub fn new(files: F, directory: PathBuf, executable: PathBuf) -> Self {
        Self {
            files,
            directory,
            executable,
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(FILE_NAME)
    }
}

impl DesktopAutostart<FsAutostartFiles> {
    /// Com o disco de verdade, a pasta de config do usuário (`$XDG_CONFIG_HOME`) e o executável
    /// que está rodando (`/usr/bin/elgin-tray` no pacote AUR).
    pub fn for_current_exe() -> io::Result<Self> {
        let config = directories::BaseDirs::new()
            .ok_or_else(|| io::Error::other("pasta pessoal não encontrada"))?
            .config_dir()
            .to_path_buf();
        Ok(Self::new(
            FsAutostartFiles,
            config.join("autostart"),
            std::env::current_exe()?,
        ))
    }
}

impl<F: AutostartFiles> AutostartRegistrar for DesktopAutostart<F> {
    fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let path = self.path();
        let result = if enabled {
            self.files.write(&path, &desktop_entry(&self.executable))
        } else {
            self.files.remove(&path)
        };
        result.map_err(|error| format!("{}: {error}", path.display()))
    }
}

/// O conteúdo do `.desktop`.
fn desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Elgin Tray\n\
         Comment=Controle da lâmpada Elgin Smart pela bandeja\n\
         Exec={}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        quote_exec(&executable.to_string_lossy())
    )
}

/// Um argumento de `Exec=` entre aspas, com os escapes da especificação Desktop Entry (os de
/// aspas e depois os de string, por isso a barra invertida vira quatro).
fn quote_exec(argument: &str) -> String {
    let mut quoted = String::from("\"");
    for character in argument.chars() {
        match character {
            '\\' => quoted.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                quoted.push_str(r"\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// O disco de verdade.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsAutostartFiles;

impl AutostartFiles for FsAutostartFiles {
    fn write(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        std::fs::write(path, contents)
    }

    fn remove(&mut self, path: &Path) -> io::Result<()> {
        match std::fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Debug, Default)]
    struct FakeFiles {
        files: HashMap<PathBuf, String>,
        broken: bool,
    }

    impl AutostartFiles for FakeFiles {
        fn write(&mut self, path: &Path, contents: &str) -> io::Result<()> {
            if self.broken {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            self.files.insert(path.to_path_buf(), contents.to_owned());
            Ok(())
        }

        fn remove(&mut self, path: &Path) -> io::Result<()> {
            self.files.remove(path);
            Ok(())
        }
    }

    fn autostart() -> DesktopAutostart<FakeFiles> {
        DesktopAutostart::new(
            FakeFiles::default(),
            PathBuf::from("/home/ana/.config/autostart"),
            PathBuf::from("/usr/bin/elgin-tray"),
        )
    }

    fn entry_path() -> PathBuf {
        PathBuf::from("/home/ana/.config/autostart/elgin-tray.desktop")
    }

    #[test]
    fn enabling_writes_a_desktop_entry_for_the_executable() {
        let mut autostart = autostart();

        autostart.set_enabled(true).unwrap();

        let entry = &autostart.files.files[&entry_path()];
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("\nType=Application\n"));
        assert!(entry.contains("\nExec=\"/usr/bin/elgin-tray\"\n"));
    }

    #[test]
    fn disabling_removes_the_entry() {
        let mut autostart = autostart();
        autostart.set_enabled(true).unwrap();

        autostart.set_enabled(false).unwrap();

        assert!(autostart.files.files.is_empty());
    }

    #[test]
    fn exec_quoting_follows_the_spec() {
        assert_eq!(quote_exec("/opt/my app/elgin"), r#""/opt/my app/elgin""#);
        assert_eq!(quote_exec("/a$b`c\"d"), r#""/a\\$b\\`c\\"d""#);
        assert_eq!(quote_exec(r"/a\b"), r#""/a\\\\b""#);
        assert_eq!(quote_exec("/100%"), r#""/100%%""#);
    }

    #[test]
    fn file_errors_are_reported_with_the_path() {
        let mut autostart = autostart();
        autostart.files.broken = true;

        let error = autostart.set_enabled(true).unwrap_err();
        assert!(error.contains("elgin-tray.desktop"));
    }
}
