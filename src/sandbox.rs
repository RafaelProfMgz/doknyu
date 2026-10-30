//! Detection of the Flatpak sandbox. Pure and iced-free.
//!
//! Inside Flatpak there is no `docker` CLI, no terminal emulator of the host and no `DOCKER_HOST`;
//! the app adapts (hides the external terminal, explains how to open the Docker socket).

use std::path::Path;
use std::sync::OnceLock;

/// File Flatpak creates at the root of every sandbox.
const FLATPAK_INFO: &str = ".flatpak-info";

/// True when the app runs inside a Flatpak sandbox (cached: it cannot change while running).
pub fn in_flatpak() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| in_flatpak_at(Path::new("/")))
}

/// Same check against another root directory (testable).
pub fn in_flatpak_at(root: &Path) -> bool {
    root.join(FLATPAK_INFO).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_flatpak_info() {
        let dir = std::env::temp_dir().join(format!("doknyu-sandbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!in_flatpak_at(&dir));
        std::fs::write(dir.join(".flatpak-info"), "[Application]\n").unwrap();
        assert!(in_flatpak_at(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
