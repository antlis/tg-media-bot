//! Minimal logger: stderr plus an optional size-rotated file.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const BACKUPS: u32 = 3;

struct Sink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

struct Logger {
    level: log::LevelFilter,
    sink: Option<Mutex<Sink>>,
}

impl log::Log for Logger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        // Dependencies are chatty at debug level; only our own crate gets it.
        meta.level() <= self.level && (meta.level() <= log::Level::Info || meta.target().starts_with("tg_media_bot"))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{} {:5} {}\n", timestamp(), record.level(), record.args());
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Some(sink) = &self.sink {
            if let Ok(mut sink) = sink.lock() {
                sink.write(&line);
            }
        }
    }

    fn flush(&self) {}
}

impl Sink {
    fn open(path: PathBuf) -> Sink {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let written = file.as_ref().and_then(|f| f.metadata().ok()).map(|m| m.len()).unwrap_or(0);
        Sink { path, file, written }
    }

    fn write(&mut self, line: &str) {
        if self.written + line.len() as u64 > MAX_FILE_BYTES {
            self.rotate();
        }
        if let Some(f) = &mut self.file {
            if f.write_all(line.as_bytes()).is_ok() {
                self.written += line.len() as u64;
            }
        }
    }

    fn rotate(&mut self) {
        self.file = None;
        let name = |n: u32| PathBuf::from(format!("{}.{n}", self.path.display()));
        let _ = std::fs::remove_file(name(BACKUPS));
        for n in (1..BACKUPS).rev() {
            let _ = std::fs::rename(name(n), name(n + 1));
        }
        let _ = std::fs::rename(&self.path, name(1));
        *self = Sink::open(self.path.clone());
    }
}

pub fn init(level: log::LevelFilter, file: Option<PathBuf>) {
    let logger = Logger { level, sink: file.map(|p| Mutex::new(Sink::open(p))) };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}

/// UTC timestamp, `2026-01-31T12:34:56Z`, without pulling in a date crate.
fn timestamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::format_utc;

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_794_787_199), "2026-11-15T23:59:59Z");
    }
}
