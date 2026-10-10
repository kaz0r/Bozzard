//! Crash reporting for the editor process and a startup notice for new reports.
use bozzard_diagnostics::crash;
use std::path::PathBuf;

/// Install the panic hook before anything else can panic.
pub(crate) fn install() {
    crash::install(crash::CrashConfig::new(
        &crash::executable_name("bozzard-editor"),
        env!("CARGO_PKG_VERSION"),
        option_env!("BOZZARD_GIT_HASH"),
    ));
}

/// The notice for reports written since the previous interactive launch, recording this one.
/// Automated runs leave the record alone so they never consume a notice.
pub(crate) fn launch_notice() -> Option<String> {
    let config = crash::installed()?;
    match crash::reports_since_last_launch(&config.directory, &config.app) {
        Ok(reports) => notice(&reports),
        Err(error) => {
            eprintln!("Crash reports: {}: {error}", config.directory.display());
            None
        }
    }
}

/// One status line naming the newest report; the console lists up to eight.
pub(crate) fn notice(reports: &[PathBuf]) -> Option<String> {
    let newest = reports.first()?;
    let mut text = if reports.len() == 1 {
        format!(
            "A crash report was written since the last launch: {}",
            newest.display()
        )
    } else {
        format!(
            "{} crash reports were written since the last launch; newest: {}",
            reports.len(),
            newest.display()
        )
    };
    for path in reports.iter().skip(1).take(7) {
        text.push('\n');
        text.push_str(&path.display().to_string());
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_names_the_newest_report_and_lists_a_bounded_set() {
        assert_eq!(notice(&[]), None);
        let one = notice(&[PathBuf::from("/c/crash-a.txt")]).unwrap();
        assert_eq!(
            one,
            "A crash report was written since the last launch: /c/crash-a.txt"
        );
        let many: Vec<_> = (0..12)
            .map(|i| PathBuf::from(format!("/c/crash-{i}.txt")))
            .collect();
        let text = notice(&many).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "12 crash reports were written since the last launch; newest: /c/crash-0.txt"
        );
        assert_eq!(lines.clone().count(), 7);
        assert_eq!(lines.next(), Some("/c/crash-1.txt"));
    }
}
