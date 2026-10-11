//! Per-user data locations shared by save games, player settings and crash reports.
use std::{ffi::OsString, path::PathBuf};

/// The per-user data root: `LOCALAPPDATA`, else `XDG_DATA_HOME`, else `$HOME/.local/share`.
/// Unset and empty variables are skipped; `None` means the platform supplied none.
pub fn user_data_dir() -> Option<PathBuf> {
    data_dir_from(
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )
}

/// [`user_data_dir`] over explicit variable values, so selection is testable without the
/// process environment.
pub fn data_dir_from(
    local_app_data: Option<OsString>,
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    set(local_app_data)
        .or_else(|| set(xdg_data_home))
        .map(PathBuf::from)
        .or_else(|| set(home).map(|home| PathBuf::from(home).join(".local/share")))
}

/// The engine's directory beneath [`user_data_dir`].
pub fn engine_data_dir() -> Option<PathBuf> {
    user_data_dir().map(|root| root.join("bozzard"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_prefers_local_app_data_then_xdg_then_home_and_skips_empty_values() {
        let some = |value: &str| Some(OsString::from(value));
        assert_eq!(
            data_dir_from(some("C:/Local"), some("/xdg"), some("/home/u")),
            Some(PathBuf::from("C:/Local"))
        );
        assert_eq!(
            data_dir_from(some(""), some("/xdg"), some("/home/u")),
            Some(PathBuf::from("/xdg"))
        );
        assert_eq!(
            data_dir_from(None, some(""), some("/home/u")),
            Some(PathBuf::from("/home/u/.local/share"))
        );
        assert_eq!(data_dir_from(None, None, some("")), None);
        assert_eq!(data_dir_from(None, None, None), None);
    }
}
