//! Acesso ao sistema de arquivos, atrás de um trait para os testes não tocarem no disco.

use std::fs;
use std::io;
use std::path::Path;

/// Lê e grava arquivos de texto.
pub trait FileStore {
    /// Conteúdo do arquivo; `None` se ele não existe.
    fn read(&self, path: &Path) -> io::Result<Option<String>>;

    /// Grava de forma atômica: ou o arquivo fica com o conteúdo novo inteiro, ou continua o antigo.
    fn write_atomic(&mut self, path: &Path, contents: &str) -> io::Result<()>;
}

/// O sistema de arquivos de verdade.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsStore;

impl FileStore for FsStore {
    fn read(&self, path: &Path) -> io::Result<Option<String>> {
        match fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_atomic(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        // Arquivo temporário na mesma pasta + rename: o rename substitui o arquivo de uma vez.
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, contents)?;
        restrict_to_owner(&temporary)?;
        fs::rename(&temporary, path)
    }
}

/// O arquivo guarda a local key e a sessão da nuvem: só o dono pode ler.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// No Windows a pasta `%APPDATA%` do usuário já é acessível só por ele.
#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Arquivos em memória, com falhas roteirizadas.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeFileStore {
    inner: std::rc::Rc<std::cell::RefCell<FakeInner>>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct FakeInner {
    files: std::collections::HashMap<std::path::PathBuf, String>,
    fail_next: Option<io::ErrorKind>,
}

#[cfg(test)]
impl FakeFileStore {
    pub(crate) fn put(&self, path: &Path, contents: &str) {
        self.inner
            .borrow_mut()
            .files
            .insert(path.to_path_buf(), contents.to_owned());
    }

    pub(crate) fn get(&self, path: &Path) -> Option<String> {
        self.inner.borrow().files.get(path).cloned()
    }

    pub(crate) fn fail_next(&self, kind: io::ErrorKind) {
        self.inner.borrow_mut().fail_next = Some(kind);
    }

    fn take_failure(&self) -> io::Result<()> {
        match self.inner.borrow_mut().fail_next.take() {
            Some(kind) => Err(kind.into()),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
impl FileStore for FakeFileStore {
    fn read(&self, path: &Path) -> io::Result<Option<String>> {
        self.take_failure()?;
        Ok(self.get(path))
    }

    fn write_atomic(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        self.take_failure()?;
        self.put(path, contents);
        Ok(())
    }
}
