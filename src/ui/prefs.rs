//! Preferences kept between runs. Reading or writing them is never fatal: callers only log.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::ui::theme::ThemeMode;

/// File of the theme preference, inside the config directory.
const THEME_FILE: &str = "theme";

/// `$XDG_CONFIG_HOME/doknyu`, or `$HOME/.config/doknyu`; `None` if neither is usable.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn config_dir_from(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    // The XDG spec says a relative `XDG_CONFIG_HOME` must be ignored.
    let xdg = xdg.map(PathBuf::from).filter(|p| p.is_absolute());
    let base = match xdg {
        Some(dir) => dir,
        None => {
            let home = home.filter(|h| !h.is_empty())?;
            PathBuf::from(home).join(".config")
        }
    };
    Some(base.join("doknyu"))
}

fn to_text(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::System => "system",
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
    }
}

fn from_text(text: &str) -> Option<ThemeMode> {
    match text.trim().to_lowercase().as_str() {
        "system" => Some(ThemeMode::System),
        "dark" => Some(ThemeMode::Dark),
        "light" => Some(ThemeMode::Light),
        _ => None,
    }
}

/// The saved theme; a missing, empty or unreadable file or an unknown word gives `System`.
pub fn load_theme_mode(dir: &Path) -> ThemeMode {
    match std::fs::read_to_string(dir.join(THEME_FILE)) {
        Ok(text) => from_text(&text).unwrap_or_else(|| {
            tracing::warn!("ignoring the saved theme: unknown value");
            ThemeMode::System
        }),
        // Nothing saved yet is the normal first run.
        Err(e) if e.kind() == io::ErrorKind::NotFound => ThemeMode::System,
        Err(e) => {
            tracing::warn!("could not read the saved theme: {e}");
            ThemeMode::System
        }
    }
}

/// Saves the theme, creating the directory if needed.
pub fn save_theme_mode(dir: &Path, mode: ThemeMode) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(THEME_FILE), to_text(mode))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory path that does not exist yet and is unique to this call.
    pub(crate) fn unique_dir(name: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "doknyu-test-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn theme_pref_roundtrip() {
        let dir = unique_dir("roundtrip");
        save_theme_mode(&dir, ThemeMode::Light).unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::Light);
        save_theme_mode(&dir, ThemeMode::Dark).unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::Dark);
        save_theme_mode(&dir, ThemeMode::System).unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::System);
        std::fs::write(dir.join("theme"), "azul").unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::System);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            load_theme_mode(&dir),
            ThemeMode::System,
            "missing directory"
        );
    }

    #[test]
    fn theme_pref_tolerates_whitespace_case_and_empty_files() {
        let dir = unique_dir("lenient");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("theme");
        std::fs::write(&file, "  Dark\n").unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::Dark);
        std::fs::write(&file, "").unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::System);
        std::fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::System, "not UTF-8");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn theme_pref_unreadable_is_system_and_unwritable_is_an_error() {
        let dir = unique_dir("unreadable");
        // `theme` is a directory: reading it fails; so does writing over it.
        std::fs::create_dir_all(dir.join("theme")).unwrap();
        assert_eq!(load_theme_mode(&dir), ThemeMode::System);
        assert!(save_theme_mode(&dir, ThemeMode::Dark).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn config_dir_prefers_xdg_then_home() {
        let os = |s: &str| Some(OsString::from(s));
        assert_eq!(
            config_dir_from(os("/xdg"), os("/home/u")),
            Some(PathBuf::from("/xdg/doknyu"))
        );
        assert_eq!(
            config_dir_from(None, os("/home/u")),
            Some(PathBuf::from("/home/u/.config/doknyu"))
        );
        // An empty or relative XDG_CONFIG_HOME is ignored, as the spec says.
        assert_eq!(
            config_dir_from(os(""), os("/home/u")),
            Some(PathBuf::from("/home/u/.config/doknyu"))
        );
        assert_eq!(
            config_dir_from(os("rel"), os("/home/u")),
            Some(PathBuf::from("/home/u/.config/doknyu"))
        );
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(os(""), os("")), None);
    }
}
