//! Log em arquivo do daemon. Nunca registrar segredos (local key, tokens, user code).
//!
//! - Windows: `%LOCALAPPDATA%\elgin-tray\logs\elgin-tray.log`
//! - Linux: `~/.local/state/elgin-tray/elgin-tray.log`
//!
//! Ao iniciar, um log maior que [`MAX_LOG_BYTES`] vira `elgin-tray.log.old` (só uma cópia antiga).
//! Em builds de desenvolvimento o log também vai para o terminal.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

pub(crate) const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// Liga o log. Devolve o caminho do arquivo, se deu para abrir.
pub(crate) fn init() -> Option<PathBuf> {
    let path = log_dir()?.join("elgin-tray.log");
    let file = open(&path);
    let logger = FileLogger {
        file: Mutex::new(file),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    Some(path)
}

fn log_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let dir = match base.state_dir() {
        Some(state) => state.join("elgin-tray"),
        None => base.data_local_dir().join("elgin-tray").join("logs"),
    };
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn open(path: &PathBuf) -> Option<File> {
    let size = fs::metadata(path).map_or(0, |metadata| metadata.len());
    if should_rotate(size) {
        let _ = fs::rename(path, path.with_extension("log.old"));
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// O arquivo atual passou do tamanho máximo?
pub(crate) fn should_rotate(size: u64) -> bool {
    size > MAX_LOG_BYTES
}

struct FileLogger {
    file: Mutex<Option<File>>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}\n",
            utc_timestamp(SystemTime::now()),
            record.level(),
            record.args()
        );
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.write_all(line.as_bytes());
        }
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

/// `AAAA-MM-DD HH:MM:SS` em UTC (sem dependência de fuso).
pub(crate) fn utc_timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Data do calendário a partir de dias desde 1970-01-01 (algoritmo de Howard Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rotates_only_above_the_limit() {
        assert!(!should_rotate(0));
        assert!(!should_rotate(MAX_LOG_BYTES));
        assert!(should_rotate(MAX_LOG_BYTES + 1));
    }

    #[test]
    fn formats_utc_timestamps() {
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);

        assert_eq!(utc_timestamp(at(0)), "1970-01-01 00:00:00");
        assert_eq!(utc_timestamp(at(1_700_000_000)), "2023-11-14 22:13:20");
        assert_eq!(utc_timestamp(at(1_709_208_000)), "2024-02-29 12:00:00");
        assert_eq!(utc_timestamp(at(1_798_761_599)), "2026-12-31 23:59:59");
    }
}
