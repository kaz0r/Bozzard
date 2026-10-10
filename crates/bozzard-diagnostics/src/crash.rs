//! Panic reports for the player, editor and server.
//!
//! [`install`] adds one process-wide panic hook. Each panic writes a plain-text report (app,
//! version, OS, thread, message, location, backtrace and the newest log lines) to the crash
//! directory, prints its path, then runs the previously installed hook. The hook never panics:
//! every write is fallible and a failed report only loses the report.
use std::{
    backtrace::Backtrace,
    collections::VecDeque,
    ffi::OsString,
    fmt::{self, Write as _},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, OnceLock, TryLockError,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Newest log lines kept for a report.
pub const RECENT_LINES: usize = 64;
/// Longest kept log line in bytes; longer lines are cut at a character boundary.
pub const MAX_LINE_BYTES: usize = 512;
/// Reports one process writes. Later panics only run the previous hook.
pub const MAX_REPORTS_PER_PROCESS: u32 = 8;
/// Reports kept in the directory; [`install`] removes older ones.
pub const KEEP_REPORTS: usize = 32;
/// Environment override for the crash directory.
pub const DIRECTORY_VARIABLE: &str = "BOZZARD_CRASH_DIR";
const PREFIX: &str = "crash-";
const EXTENSION: &str = ".txt";

/// A bounded ring of log lines. A full ring reuses its oldest line's buffer, so steady-state
/// recording does not allocate.
pub struct LogRing {
    lines: VecDeque<String>,
    capacity: usize,
    dropped: u64,
}
impl LogRing {
    pub const fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            capacity,
            dropped: 0,
        }
    }
    /// Append one line, bounded to [`MAX_LINE_BYTES`] with control characters flattened.
    pub fn push(&mut self, line: fmt::Arguments<'_>) {
        if self.capacity == 0 {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let mut buffer = if self.lines.len() >= self.capacity {
            self.dropped = self.dropped.saturating_add(1);
            let mut reused = self.lines.pop_front().unwrap_or_default();
            reused.clear();
            reused
        } else {
            String::new()
        };
        // A Display error only shortens this line.
        let _ = Bounded {
            text: &mut buffer,
            full: false,
        }
        .write_fmt(line);
        self.lines.push_back(buffer);
    }
    pub fn len(&self) -> usize {
        self.lines.len()
    }
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
    /// Lines pushed out by newer ones.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    /// Oldest first.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }
}

/// Appends up to [`MAX_LINE_BYTES`], replacing line breaks and tabs with spaces.
struct Bounded<'a> {
    text: &'a mut String,
    full: bool,
}
impl fmt::Write for Bounded<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        const ELLIPSIS: char = '…';
        for c in s.chars() {
            if self.full {
                return Err(fmt::Error);
            }
            let c = if c.is_control() { ' ' } else { c };
            if self.text.len() + c.len_utf8() > MAX_LINE_BYTES - ELLIPSIS.len_utf8() {
                self.text.push(ELLIPSIS);
                self.full = true;
                return Err(fmt::Error);
            }
            self.text.push(c);
        }
        Ok(())
    }
}

static RECENT: Mutex<LogRing> = Mutex::new(LogRing::new(RECENT_LINES));
static START: OnceLock<Instant> = OnceLock::new();
static CONFIG: OnceLock<CrashConfig> = OnceLock::new();
static APP_NAME: OnceLock<String> = OnceLock::new();
static REPORTS: AtomicU32 = AtomicU32::new(0);

fn uptime() -> Duration {
    START.get_or_init(Instant::now).elapsed()
}

/// Remember a log line for the next crash report, stamped with process uptime.
pub fn record(level: &str, source: &str, message: &str) {
    let seconds = uptime().as_secs_f64();
    RECENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(format_args!(
            "[{seconds:>10.3}s] {level} {source}: {message}"
        ));
}

/// A copy of the remembered lines, oldest first.
pub fn recent_lines() -> Vec<String> {
    let ring = RECENT.lock().unwrap_or_else(|error| error.into_inner());
    ring.lines().map(str::to_owned).collect()
}

/// Where and as whom this process reports panics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashConfig {
    /// Report name prefix and header, e.g. `bozzard-editor` or an exported game's executable.
    pub app: String,
    pub version: String,
    pub git: Option<String>,
    pub directory: PathBuf,
}
impl CrashConfig {
    /// A configuration using [`default_directory`].
    pub fn new(app: &str, version: &str, git: Option<&str>) -> Self {
        Self {
            app: app.to_owned(),
            version: version.to_owned(),
            git: git.filter(|git| !git.is_empty()).map(str::to_owned),
            directory: default_directory(),
        }
    }
}

/// [`DIRECTORY_VARIABLE`], else `<user data>/bozzard/crashes`, else the temporary directory.
pub fn default_directory() -> PathBuf {
    directory_from(
        std::env::var_os(DIRECTORY_VARIABLE),
        crate::paths::engine_data_dir(),
        std::env::temp_dir(),
    )
}

/// [`default_directory`] over explicit inputs, so selection is testable.
pub fn directory_from(
    override_directory: Option<OsString>,
    engine_data: Option<PathBuf>,
    temp: PathBuf,
) -> PathBuf {
    override_directory
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| engine_data.map(|root| root.join("crashes")))
        .unwrap_or_else(|| temp.join("bozzard-crashes"))
}

/// The running executable's file stem, which is the game's name in an exported game.
pub fn executable_name(fallback: &str) -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// Install the panic hook once per process. Later calls keep the first configuration.
/// Returns the active crash directory.
pub fn install(config: CrashConfig) -> &'static Path {
    START.get_or_init(Instant::now);
    let mut installed = false;
    let active = CONFIG.get_or_init(|| {
        installed = true;
        config
    });
    if installed {
        prune(&active.directory, KEEP_REPORTS);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(path) = report(info, active) {
                let _ = writeln!(
                    io::stderr().lock(),
                    "crash report written to {}",
                    path.display()
                );
            }
            previous(info);
        }));
    }
    &active.directory
}

/// The installed configuration, if any.
pub fn installed() -> Option<&'static CrashConfig> {
    CONFIG.get()
}

/// Name later reports after `app`, e.g. once a player knows which game it runs.
/// Only the first call takes effect.
pub fn set_app_name(app: &str) {
    let _ = APP_NAME.set(app.to_owned());
}

/// Everything one report contains. Borrowed so the hook formats without building strings.
pub struct Report<'a> {
    pub app: &'a str,
    pub version: &'a str,
    pub git: Option<&'a str>,
    /// Since the Unix epoch.
    pub time: Duration,
    pub process: u32,
    pub thread: &'a str,
    pub message: &'a str,
    /// File, line and column.
    pub location: Option<(&'a str, u32, u32)>,
    pub backtrace: &'a dyn fmt::Display,
    pub recent: Option<&'a LogRing>,
}

/// Write the plain-text report body.
pub fn write_report(out: &mut impl Write, report: &Report<'_>) -> io::Result<()> {
    writeln!(out, "Bozzard crash report")?;
    writeln!(out, "app: {}", report.app)?;
    writeln!(out, "version: {}", report.version)?;
    writeln!(out, "git: {}", report.git.unwrap_or("unknown"))?;
    writeln!(
        out,
        "time: {} (unix {})",
        Utc(report.time.as_secs()),
        report.time.as_secs()
    )?;
    writeln!(
        out,
        "os: {} {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY
    )?;
    writeln!(out, "process: {}", report.process)?;
    writeln!(out, "thread: {}", report.thread)?;
    writeln!(out, "message: {}", report.message)?;
    match report.location {
        Some((file, line, column)) => writeln!(out, "location: {file}:{line}:{column}")?,
        None => writeln!(out, "location: unknown")?,
    }
    writeln!(out, "\nbacktrace:\n{}", report.backtrace)?;
    match report.recent {
        Some(ring) => {
            writeln!(
                out,
                "\nrecent log ({} lines, {} older dropped):",
                ring.len(),
                ring.dropped()
            )?;
            for line in ring.lines() {
                writeln!(out, "{line}")?;
            }
        }
        None => writeln!(out, "\nrecent log: unavailable (log buffer busy)")?,
    }
    Ok(())
}

/// `crash-<app>-<UTC time>-<process>-<serial>.txt`, with the app reduced to `[A-Za-z0-9_-]`.
pub fn report_file_name(app: &str, time: Duration, process: u32, serial: u32) -> String {
    let mut name = String::with_capacity(96);
    name.push_str(PREFIX);
    push_slug(&mut name, app);
    let _ = write!(
        name,
        "-{}-{process}-{serial}{EXTENSION}",
        Compact(time.as_secs())
    );
    name
}

fn push_slug(out: &mut String, app: &str) {
    let start = out.len();
    for c in app.chars().take(48) {
        out.push(if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            c
        } else {
            '-'
        });
    }
    if out.len() == start {
        out.push_str("app");
    }
}

/// Write one report file in `directory`, creating it if needed.
pub fn write_report_file(
    directory: &Path,
    report: &Report<'_>,
    serial: u32,
) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let path = directory.join(report_file_name(
        report.app,
        report.time,
        report.process,
        serial,
    ));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let mut out = io::BufWriter::new(file);
    write_report(&mut out, report)?;
    out.into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()?;
    Ok(path)
}

/// Report files in `directory`, newest first. A missing directory has none.
pub fn reports(directory: &Path) -> io::Result<Vec<(SystemTime, PathBuf)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(PREFIX) || !name.ends_with(EXTENSION) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            found.push((metadata.modified()?, entry.path()));
        }
    }
    found.sort_by(|a, b| b.cmp(a));
    Ok(found)
}

/// Remove all but the newest `keep` reports. Failures leave files in place.
pub fn prune(directory: &Path, keep: usize) {
    if let Ok(found) = reports(directory) {
        for (_, path) in found.into_iter().skip(keep) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Reports written since `app` last called this, newest first, then record this launch.
/// The first launch only records itself.
pub fn reports_since_last_launch(directory: &Path, app: &str) -> io::Result<Vec<PathBuf>> {
    let mut marker = String::from(".last-launch-");
    push_slug(&mut marker, app);
    let marker = directory.join(marker);
    let since = fs::read_to_string(&marker)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .map(|millis| UNIX_EPOCH + Duration::from_millis(millis));
    let found = match since {
        Some(since) => reports(directory)?
            .into_iter()
            .filter(|(modified, _)| *modified > since)
            .map(|(_, path)| path)
            .collect(),
        None => Vec::new(),
    };
    fs::create_dir_all(directory)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    fs::write(&marker, now.to_string())?;
    Ok(found)
}

fn report(info: &std::panic::PanicHookInfo<'_>, config: &CrashConfig) -> Option<PathBuf> {
    let serial = REPORTS.fetch_add(1, Ordering::Relaxed);
    if serial >= MAX_REPORTS_PER_PROCESS {
        return None;
    }
    let thread = std::thread::current();
    let mut thread_label = String::with_capacity(64);
    let _ = write!(
        thread_label,
        "{} ({:?})",
        thread.name().unwrap_or("<unnamed>"),
        thread.id()
    );
    let backtrace = Backtrace::force_capture();
    let recent = lock_recent();
    let report = Report {
        app: APP_NAME.get().unwrap_or(&config.app),
        version: &config.version,
        git: config.git.as_deref(),
        time: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default(),
        process: std::process::id(),
        thread: &thread_label,
        message: info
            .payload_as_str()
            .unwrap_or("<non-string panic payload>"),
        location: info
            .location()
            .map(|location| (location.file(), location.line(), location.column())),
        backtrace: &backtrace,
        recent: recent.as_deref(),
    };
    match write_report_file(&config.directory, &report, serial) {
        Ok(path) => Some(path),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "crash report could not be written to {}: {error}",
                config.directory.display()
            );
            None
        }
    }
}

/// The panicking thread may itself hold the ring (or another thread may be logging):
/// retry briefly, then report without it rather than block.
fn lock_recent() -> Option<MutexGuard<'static, LogRing>> {
    for _ in 0..64 {
        match RECENT.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(error)) => return Some(error.into_inner()),
            Err(TryLockError::WouldBlock) => std::thread::yield_now(),
        }
    }
    None
}

/// Civil UTC date from Unix seconds (Howard Hinnant's days-from-civil inverse).
fn civil(seconds: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rest / 3600) as u32,
        (rest % 3600 / 60) as u32,
        (rest % 60) as u32,
    )
}
/// ISO 8601 UTC, e.g. `2026-10-10T10:30:00Z`.
struct Utc(u64);
impl fmt::Display for Utc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, mo, d, h, mi, s) = civil(self.0);
        write!(f, "{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
    }
}
/// File-name form, e.g. `20261010T103000Z`.
struct Compact(u64);
impl fmt::Display for Compact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, mo, d, h, mi, s) = civil(self.0);
        write!(f, "{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample<'a>(ring: Option<&'a LogRing>, backtrace: &'a dyn fmt::Display) -> Report<'a> {
        Report {
            app: "First Trail",
            version: "0.1.0",
            git: Some("abc123"),
            time: Duration::from_secs(1_791_628_200),
            process: 42,
            thread: "main (ThreadId(1))",
            message: "index out of bounds",
            location: Some(("src/main.rs", 10, 5)),
            backtrace,
            recent: ring,
        }
    }

    #[test]
    fn report_lists_identity_platform_panic_backtrace_and_recent_lines() {
        let mut ring = LogRing::new(2);
        ring.push(format_args!("first"));
        ring.push(format_args!("second"));
        ring.push(format_args!("third"));
        let mut text = Vec::new();
        write_report(&mut text, &sample(Some(&ring), &"frame 0: main")).unwrap();
        let text = String::from_utf8(text).unwrap();
        for expected in [
            "Bozzard crash report\n",
            "app: First Trail\n",
            "version: 0.1.0\n",
            "git: abc123\n",
            "time: 2026-10-10T10:30:00Z (unix 1791628200)\n",
            &format!("os: {} {}", std::env::consts::OS, std::env::consts::ARCH),
            "process: 42\n",
            "thread: main (ThreadId(1))\n",
            "message: index out of bounds\n",
            "location: src/main.rs:10:5\n",
            "\nbacktrace:\nframe 0: main\n",
            "\nrecent log (2 lines, 1 older dropped):\nsecond\nthird\n",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in\n{text}");
        }
        let mut busy = Vec::new();
        let mut report = sample(None, &"");
        report.git = None;
        report.location = None;
        write_report(&mut busy, &report).unwrap();
        let busy = String::from_utf8(busy).unwrap();
        assert!(busy.contains("git: unknown\n"));
        assert!(busy.contains("location: unknown\n"));
        assert!(busy.contains("recent log: unavailable"));
    }

    #[test]
    fn utc_dates_cover_epoch_leap_days_and_file_names_are_portable() {
        assert_eq!(Utc(0).to_string(), "1970-01-01T00:00:00Z");
        assert_eq!(Utc(951_782_400).to_string(), "2000-02-29T00:00:00Z");
        assert_eq!(Utc(4_107_542_399).to_string(), "2100-02-28T23:59:59Z");
        assert_eq!(
            report_file_name(
                "First Trail: ü/..",
                Duration::from_secs(1_791_628_200),
                7,
                2
            ),
            "crash-First-Trail-------20261010T103000Z-7-2.txt"
        );
        assert_eq!(
            report_file_name("", Duration::ZERO, 1, 0),
            "crash-app-19700101T000000Z-1-0.txt"
        );
    }

    #[test]
    fn ring_keeps_newest_lines_bounds_each_line_and_reuses_buffers() {
        let mut ring = LogRing::new(3);
        for i in 0..10 {
            ring.push(format_args!("line {i}"));
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.dropped(), 7);
        assert_eq!(
            ring.lines().collect::<Vec<_>>(),
            ["line 7", "line 8", "line 9"]
        );
        let long = "é".repeat(MAX_LINE_BYTES);
        ring.push(format_args!("multi\nline\t{long}"));
        let last = ring.lines().last().unwrap();
        assert!(last.len() <= MAX_LINE_BYTES);
        assert!(last.starts_with("multi line é"));
        assert!(last.ends_with('…'));
        let mut empty = LogRing::new(0);
        empty.push(format_args!("lost"));
        assert!(empty.is_empty());
        assert_eq!(empty.dropped(), 1);
    }

    #[test]
    fn crash_directory_prefers_override_then_engine_data_then_temp() {
        let temp = PathBuf::from("/tmp");
        assert_eq!(
            directory_from(
                Some("/x".into()),
                Some("/data/bozzard".into()),
                temp.clone()
            ),
            PathBuf::from("/x")
        );
        assert_eq!(
            directory_from(Some("".into()), Some("/data/bozzard".into()), temp.clone()),
            PathBuf::from("/data/bozzard/crashes")
        );
        assert_eq!(
            directory_from(None, None, temp),
            PathBuf::from("/tmp/bozzard-crashes")
        );
    }

    #[test]
    fn report_files_are_listed_newest_first_pruned_and_noticed_once_per_launch() {
        let directory = std::env::temp_dir().join(format!(
            "bozzard-crash-listing-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&directory);
        assert!(reports(&directory).unwrap().is_empty());
        assert!(
            reports_since_last_launch(&directory, "editor")
                .unwrap()
                .is_empty(),
            "the first launch only records itself"
        );
        std::thread::sleep(Duration::from_millis(20));
        let mut written = Vec::new();
        for serial in 0..3 {
            let mut report = sample(None, &"");
            report.time = Duration::from_secs(u64::from(serial));
            written.push(write_report_file(&directory, &report, serial).unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
        fs::write(directory.join("notes.txt"), "not a report").unwrap();
        let listed: Vec<_> = reports(&directory)
            .unwrap()
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        assert_eq!(listed, written.iter().rev().cloned().collect::<Vec<_>>());
        assert_eq!(
            reports_since_last_launch(&directory, "editor").unwrap(),
            listed
        );
        assert!(
            reports_since_last_launch(&directory, "editor")
                .unwrap()
                .is_empty()
        );
        prune(&directory, 1);
        assert_eq!(
            reports(&directory)
                .unwrap()
                .into_iter()
                .map(|(_, p)| p)
                .collect::<Vec<_>>(),
            [written[2].clone()]
        );
        assert!(directory.join("notes.txt").exists());
        fs::remove_dir_all(&directory).unwrap();
    }
}
