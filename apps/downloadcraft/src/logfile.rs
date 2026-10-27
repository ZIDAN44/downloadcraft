//! The log file, for working out what happened when a download misbehaves (a server's answers,
//! retries, parts handed back, the graphics adapter). `downloadcraft.log` in the data folder: a
//! new one each time DownloadCraft starts (the last run's renamed `downloadcraft.old.log`), up to
//! [`MAX`] a run. DownloadCraft's own lines from Info up, other libraries' warnings.
//!
//! From MailCraft (`apps/mailcraft/src/logfile.rs`).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Metadata, Record};

/// Most written in one run (the rest of the run goes unlogged, and says so).
const MAX: u64 = 4 << 20;

pub const NAME: &str = "downloadcraft.log";
const OLD: &str = "downloadcraft.old.log";

struct FileLog {
    /// The open file and how much has gone into it.
    out: Mutex<Option<(File, u64)>>,
    /// Lines also go to the terminal (a debug build, or `--log`).
    echo: bool,
}

/// A line to the terminal; a terminal gone (standard error closed, or a broken pipe once the
/// program that started DownloadCraft has ended) is passed over. `eprint!` panicked there in
/// MailCraft, and the next line logged ended it.
fn echo(to: &mut dyn Write, line: &str) {
    let _ = to.write_all(line.as_bytes());
}

impl FileLog {
    fn write_line(&self, line: &str) {
        if self.echo {
            echo(&mut std::io::stderr(), line);
        }
        let mut out = self.out.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((file, written)) = out.as_mut() else { return };
        if *written >= MAX {
            return;
        }
        let bytes = line.as_bytes();
        let ok = file.write_all(bytes).is_ok();
        *written = written.saturating_add(bytes.len() as u64);
        if !ok {
            *out = None;
        } else if *written >= MAX {
            let _ = file.write_all(b"... the log is full: the rest of this run isn't written\n");
            let _ = file.flush();
        }
    }
}

impl log::Log for FileLog {
    fn enabled(&self, m: &Metadata) -> bool {
        enabled(m.target(), m.level())
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        self.write_line(&line(&stamp(), r.level(), r.target(), &r.args().to_string()));
    }

    fn flush(&self) {
        if let Some((f, _)) = self.out.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
            let _ = f.flush();
        }
    }
}

/// Whether a line is kept: DownloadCraft's from Info up, everyone else's warnings.
fn enabled(target: &str, level: Level) -> bool {
    if target.starts_with("downloadcraft") { level <= Level::Info } else { level <= Level::Warn }
}

/// One line as written: when (UTC), how serious, where from, what (a message over several lines
/// indented under it).
fn line(stamp: &str, level: Level, target: &str, msg: &str) -> String {
    let msg = msg.trim_end().replace("\r\n", "\n").replace('\n', "\n    ");
    format!("{stamp} {level:<5} {target}: {msg}\n")
}

/// Now, as `2026-10-19 14:03:07` (UTC).
fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let secs = i64::try_from(secs).unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60)
}

/// The calendar date of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Start logging into `dir` (the previous run's log kept as the old one). Without a folder, or
/// when the file can't be made, lines go to the terminal only if `echo`.
pub fn start(dir: Option<&Path>, echo: bool) -> Option<PathBuf> {
    let path = dir.map(|d| d.join(NAME));
    let file = path.as_ref().and_then(|p| {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
            let _ = std::fs::rename(p, d.join(OLD));
        }
        File::create(p).ok()
    });
    let opened = file.is_some();
    let logger: &'static FileLog = Box::leak(Box::new(FileLog { out: Mutex::new(file.map(|f| (f, 0))), echo }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    path.filter(|_| opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A terminal that's gone doesn't end DownloadCraft (found in MailCraft when its starter exited).
    #[test]
    fn a_closed_terminal_is_passed_over() {
        struct Gone;
        impl Write for Gone {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
        }
        echo(
            &mut Gone, "a line
",
        );
        let log = FileLog { out: Mutex::new(None), echo: true };
        log.write_line(
            "still here
",
        );
    }

    #[test]
    fn lines_and_levels() {
        assert!(enabled("downloadcraft_net::http", Level::Info));
        assert!(!enabled("downloadcraft_net::http", Level::Debug));
        assert!(!enabled("rustls::client", Level::Info));
        assert!(enabled("rustls::client", Level::Warn));
        assert_eq!(line("2026-10-19 14:03:07", Level::Warn, "x", "a\r\nb\n"), "2026-10-19 14:03:07 WARN  x: a\n    b\n");
        let s = stamp();
        assert_eq!((s.len(), s.as_bytes()[4], s.as_bytes()[13]), (19, b'-', b':'));
    }

    #[test]
    fn days_become_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_753), (2026, 10, 27));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn a_full_log_stops_and_says_so() {
        let dir = std::env::temp_dir().join(format!("downloadcraft-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(NAME);
        let log = FileLog { out: Mutex::new(Some((File::create(&path).unwrap(), MAX - 10))), echo: false };
        log.write_line("0123456789abcdef\n");
        log.write_line("never written\n");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("0123456789abcdef\n... the log is full"), "{text}");
        assert!(!text.contains("never written"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
