//! Persistent diagnostics for fixed messages and non-sensitive probe metadata.
//! Never pass subprocess output, entry text, PINs, card identifiers, or key IDs.
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 1024 * 1024;
static LOG: OnceLock<Mutex<Option<Log>>> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();

struct Log {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}

impl Log {
    fn open(path: PathBuf, limit: u64) -> io::Result<Self> {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
        builder.create(parent)?;
        let file = open_file(&path)?;
        let bytes = file.metadata()?.len();
        Ok(Self { path, file: Some(file), bytes, limit })
    }

    fn append(&mut self, line: &str) -> io::Result<()> {
        if self.bytes > 0 && self.bytes.saturating_add(line.len() as u64) > self.limit {
            self.file = None;
            let mut previous = self.path.as_os_str().to_os_string();
            previous.push(".1");
            fs::rename(&self.path, PathBuf::from(previous))?;
            self.file = Some(open_file(&self.path)?);
            self.bytes = 0;
        }
        self.file.as_mut().ok_or_else(|| io::Error::other("log closed"))?.write_all(line.as_bytes())?;
        self.bytes += line.len() as u64;
        Ok(())
    }
}

fn open_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    options.open(path)
}

pub(crate) fn init() {
    STARTED.get_or_init(Instant::now);
    let path = std::env::var_os("GPUI_PASS_LOG").filter(|p| !p.is_empty()).map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_STATE_HOME").filter(|p| !p.is_empty()).map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
                .map(|p| p.join("gpui-pass/diagnostics.log"))
        });
    let sink = path.and_then(|path| match Log::open(path.clone(), MAX_LOG_BYTES) {
        Ok(log) => { eprintln!("[diagnostics] Log: {}", path.display()); Some(log) },
        Err(_) => { eprintln!("[diagnostics] File logging unavailable; using stderr."); None },
    });
    let _ = LOG.set(Mutex::new(sink));
    event(format_args!("session.start version={}", env!("CARGO_PKG_VERSION")));
}

pub(crate) fn event(message: fmt::Arguments<'_>) {
    let elapsed = STARTED.get_or_init(Instant::now).elapsed().as_millis();
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let message: String = message.to_string().chars().filter(|c| !c.is_control()).take(2048).collect();
    let line = format!("{timestamp} pid={} +{elapsed}ms {message}\n", std::process::id());
    eprint!("{line}");
    if let Some(log) = LOG.get() {
        let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
        if log.as_mut().is_some_and(|log| log.append(&line).is_err()) {
            *log = None;
            eprintln!("[diagnostics] File logging stopped after a write failure.");
        }
    }
}

macro_rules! trace {
    ($($arg:tt)*) => { $crate::diagnostics::event(format_args!($($arg)*)) };
}
pub(crate) use trace;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_are_immediately_readable_rotated_and_private() {
        let root = std::env::temp_dir().join(format!("gpui-pass-log-test-{}", std::process::id()));
        let path = root.join("diagnostics.log");
        let mut log = Log::open(path.clone(), 16).unwrap();
        log.append("first event\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first event\n");
        log.append("second event\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second event\n");
        assert_eq!(fs::read_to_string(root.join("diagnostics.log.1")).unwrap(), "first event\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        drop(log);
        fs::remove_dir_all(root).unwrap();
    }
}
