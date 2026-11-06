//! Removing doknyu from the app: what to do for each kind of install ([`plan`]) and doing it
//! ([`execute`]). Pure and iced-free. Never touches Docker (containers, images, volumes,
//! networks): only the app's own files, its package, and its preferences if asked.

use std::path::{Path, PathBuf};

use crate::install::{InstallKind, Runner};

/// App id: Flatpak name, `.desktop` and metainfo file names.
pub const APP_ID: &str = "io.github.rafaelprofmgz.Doknyu";

/// What uninstalling means for this install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallPlan {
    /// The app removes these files of the user itself (no root).
    RemoveFiles { files: Vec<PathBuf> },
    /// Runs through `pkexec` (the system password window).
    Privileged {
        program: &'static str,
        args: Vec<String>,
    },
    /// Only shows the command (shown and copied). Empty when the install is unknown.
    ShowCommand { command: String },
}

/// Why an uninstall did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallError {
    /// The user closed the password window (pkexec 126) or is not allowed (127).
    Cancelled,
    /// The package manager failed: the last lines it printed.
    Failed(String),
    /// Some files could not be removed (path and reason); the others were.
    Files(Vec<(PathBuf, String)>),
}

/// The `pkexec` (or `sudo`, when it is missing) command line that removes `package`.
fn remove_command(kind: &InstallKind) -> Option<(&'static str, Vec<String>)> {
    let (program, args, package): (&'static str, &[&str], &str) = match kind {
        InstallKind::Deb { package } => ("apt-get", &["remove", "-y"], package),
        InstallKind::Rpm { package } => ("dnf", &["remove", "-y"], package),
        InstallKind::Pacman { package } => ("pacman", &["-R", "--noconfirm"], package),
        _ => return None,
    };
    let mut full: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    full.push(package.to_owned());
    Some((program, full))
}

/// What uninstalling does for `kind`. `home` is the user's home directory (for the AppImage
/// menu entries); `has_pkexec` says whether the system password window is available.
pub fn plan(kind: &InstallKind, home: &Path, has_pkexec: bool) -> UninstallPlan {
    if let Some((program, args)) = remove_command(kind) {
        if has_pkexec {
            let mut full = vec![program.to_owned()];
            full.extend(args);
            return UninstallPlan::Privileged {
                program: "pkexec",
                args: full,
            };
        }
        return UninstallPlan::ShowCommand {
            command: format!("sudo {program} {}", args.join(" ")),
        };
    }
    let command = match kind {
        InstallKind::AppImage { path } => {
            let mut files = vec![path.clone()];
            files.extend(appimage_menu_entries(
                &home.join(".local/share/applications"),
                path,
            ));
            return UninstallPlan::RemoveFiles { files };
        }
        InstallKind::Flatpak => format!("flatpak uninstall --delete-data {APP_ID}"),
        InstallKind::Nix => "nix profile remove doknyu".to_owned(),
        InstallKind::Homebrew => "brew uninstall doknyu".to_owned(),
        InstallKind::Cargo => "cargo uninstall doknyu".to_owned(),
        // Handled above.
        InstallKind::Deb { .. } | InstallKind::Rpm { .. } | InstallKind::Pacman { .. } => {
            String::new()
        }
        InstallKind::Unknown { .. } => String::new(),
    };
    UninstallPlan::ShowCommand { command }
}

/// The `.desktop` files in `dir` whose `Exec=` runs exactly `appimage` (menu entries made by
/// AppImageLauncher or Gear Lever). A file of another app with a similar name is left alone.
pub fn appimage_menu_entries(dir: &Path, appimage: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let target = appimage.to_string_lossy();
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "desktop") && p.is_file())
        .filter(|p| {
            std::fs::read_to_string(p).is_ok_and(|body| {
                body.lines().any(|line| {
                    line.strip_prefix("Exec=").is_some_and(|cmd| {
                        cmd.split_whitespace()
                            .any(|word| word.trim_matches(['"', '\'']) == target)
                    })
                })
            })
        })
        .collect();
    found.sort();
    found
}

/// Carries out `plan`. With `prefs` set, that directory is removed too, but only after the
/// main removal worked. `ShowCommand` does nothing but the preferences (the user runs the
/// command). Blocking: run it off the UI thread.
pub fn execute(
    plan: &UninstallPlan,
    runner: &dyn Runner,
    prefs: Option<&Path>,
) -> Result<(), UninstallError> {
    match plan {
        UninstallPlan::RemoveFiles { files } => {
            let failed: Vec<(PathBuf, String)> = files
                .iter()
                .filter_map(|f| match std::fs::remove_file(f) {
                    Ok(()) => None,
                    Err(e) => Some((f.clone(), e.to_string())),
                })
                .collect();
            if !failed.is_empty() {
                return Err(UninstallError::Files(failed));
            }
        }
        UninstallPlan::Privileged { program, args } => {
            let out = runner
                .run(program, args)
                .map_err(|e| UninstallError::Failed(e.to_string()))?;
            match out.code {
                0 => {}
                126 | 127 => return Err(UninstallError::Cancelled),
                _ => return Err(UninstallError::Failed(out.tail(5))),
            }
        }
        UninstallPlan::ShowCommand { .. } => {}
    }
    if let Some(dir) = prefs {
        remove_prefs(dir)?;
    }
    Ok(())
}

/// Removes the preferences directory; already gone is fine.
fn remove_prefs(dir: &Path) -> Result<(), UninstallError> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(UninstallError::Files(vec![(
            dir.to_path_buf(),
            e.to_string(),
        )])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::fakes::{FakeRunner, temp_dir};

    fn deb() -> InstallKind {
        InstallKind::Deb {
            package: "doknyu".into(),
        }
    }

    #[test]
    fn packages_use_pkexec() {
        let home = Path::new("/home/u");
        let cases = [
            (deb(), vec!["apt-get", "remove", "-y", "doknyu"]),
            (
                InstallKind::Rpm {
                    package: "doknyu".into(),
                },
                vec!["dnf", "remove", "-y", "doknyu"],
            ),
            (
                InstallKind::Pacman {
                    package: "doknyu-bin".into(),
                },
                vec!["pacman", "-R", "--noconfirm", "doknyu-bin"],
            ),
        ];
        for (kind, args) in cases {
            assert_eq!(
                plan(&kind, home, true),
                UninstallPlan::Privileged {
                    program: "pkexec",
                    args: args.iter().map(|a| (*a).to_owned()).collect(),
                }
            );
        }
    }

    #[test]
    fn packages_without_pkexec_show_sudo() {
        assert_eq!(
            plan(&deb(), Path::new("/home/u"), false),
            UninstallPlan::ShowCommand {
                command: "sudo apt-get remove -y doknyu".into()
            }
        );
    }

    #[test]
    fn other_installs_show_their_command() {
        let home = Path::new("/home/u");
        let cases = [
            (
                InstallKind::Flatpak,
                "flatpak uninstall --delete-data io.github.rafaelprofmgz.Doknyu",
            ),
            (InstallKind::Nix, "nix profile remove doknyu"),
            (InstallKind::Homebrew, "brew uninstall doknyu"),
            (InstallKind::Cargo, "cargo uninstall doknyu"),
            (InstallKind::Unknown { exe: "/x".into() }, ""),
        ];
        for (kind, command) in cases {
            assert_eq!(
                plan(&kind, home, true),
                UninstallPlan::ShowCommand {
                    command: command.into()
                }
            );
        }
    }

    #[test]
    fn appimage_takes_only_its_own_menu_entries() {
        let home = temp_dir("uninstall-plan");
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        let image = home.join("Apps/doknyu.AppImage");
        std::fs::write(
            apps.join("appimagekit-doknyu.desktop"),
            format!("[Desktop Entry]\nExec=\"{}\" %U\n", image.display()),
        )
        .unwrap();
        // Same prefix, another file: not ours.
        std::fs::write(
            apps.join("other.desktop"),
            format!("[Desktop Entry]\nExec={}2 %U\n", image.display()),
        )
        .unwrap();
        std::fs::write(apps.join("notes.txt"), format!("Exec={}", image.display())).unwrap();
        assert_eq!(
            plan(
                &InstallKind::AppImage {
                    path: image.clone()
                },
                &home,
                true
            ),
            UninstallPlan::RemoveFiles {
                files: vec![image, apps.join("appimagekit-doknyu.desktop")]
            }
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn remove_files_then_prefs() {
        let dir = temp_dir("uninstall-files");
        let image = dir.join("doknyu.AppImage");
        let prefs = dir.join("config");
        std::fs::write(&image, b"x").unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::write(prefs.join("theme"), b"dark").unwrap();
        let plan = UninstallPlan::RemoveFiles {
            files: vec![image.clone()],
        };
        assert_eq!(execute(&plan, &FakeRunner::default(), Some(&prefs)), Ok(()));
        assert!(!image.exists());
        assert!(!prefs.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_file_removal_keeps_prefs_and_says_which() {
        let dir = temp_dir("uninstall-missing");
        let prefs = dir.join("config");
        std::fs::create_dir_all(&prefs).unwrap();
        let missing = dir.join("gone.AppImage");
        let plan = UninstallPlan::RemoveFiles {
            files: vec![missing.clone()],
        };
        match execute(&plan, &FakeRunner::default(), Some(&prefs)) {
            Err(UninstallError::Files(failed)) => assert_eq!(failed[0].0, missing),
            other => panic!("{other:?}"),
        }
        assert!(prefs.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn privileged_codes() {
        let dir = temp_dir("uninstall-priv");
        let prefs = dir.join("config");
        let plan = plan(&deb(), Path::new("/home/u"), true);
        for (code, expected) in [
            (126, Err(UninstallError::Cancelled)),
            (127, Err(UninstallError::Cancelled)),
            (
                1,
                Err(UninstallError::Failed(
                    "pkexec failed\nlast line of pkexec".into(),
                )),
            ),
        ] {
            std::fs::create_dir_all(&prefs).unwrap();
            let run = FakeRunner::default();
            run.answer("pkexec", code, "");
            assert_eq!(execute(&plan, &run, Some(&prefs)), expected);
            assert!(prefs.exists(), "prefs kept after code {code}");
        }
        let run = FakeRunner::default();
        run.answer("pkexec", 0, "");
        assert_eq!(execute(&plan, &run, Some(&prefs)), Ok(()));
        assert!(!prefs.exists());
        assert_eq!(
            run.calls(),
            vec![(
                "pkexec".to_owned(),
                ["apt-get", "remove", "-y", "doknyu"]
                    .map(String::from)
                    .to_vec()
            )]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn show_command_only_removes_prefs_when_asked() {
        let dir = temp_dir("uninstall-show");
        let prefs = dir.join("config");
        std::fs::create_dir_all(&prefs).unwrap();
        let plan = UninstallPlan::ShowCommand {
            command: "brew uninstall doknyu".into(),
        };
        let run = FakeRunner::default();
        assert_eq!(execute(&plan, &run, None), Ok(()));
        assert!(prefs.exists());
        assert_eq!(execute(&plan, &run, Some(&prefs)), Ok(()));
        assert!(!prefs.exists());
        assert!(run.calls().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
