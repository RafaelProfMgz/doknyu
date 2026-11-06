//! How this copy of doknyu was installed, and running the system programs that the "update" and
//! "uninstall" dialogs need. Pure and iced-free; the system is behind two traits ([`Env`] and
//! [`Runner`]) so the tests can fake it.
//!
//! Programs always run as `Command::new(program).args([...])`, never through a shell, and the
//! package names that come back from a package manager are validated before they are used.

use std::path::{Path, PathBuf};
use std::process::Command;

/// How the running executable got onto the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// An AppImage, at the path in `$APPIMAGE`.
    AppImage {
        path: PathBuf,
    },
    Deb {
        package: String,
    },
    Rpm {
        package: String,
    },
    /// Arch, usually from the AUR: `doknyu`, `doknyu-bin` or `doknyu-git`.
    Pacman {
        package: String,
    },
    Flatpak,
    Nix,
    Homebrew,
    /// `cargo install`, in `$CARGO_HOME/bin`.
    Cargo,
    /// None of the above (a binary copied by hand, `cargo run`...).
    Unknown {
        exe: PathBuf,
    },
}

/// What a program said when it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Exit code; -1 when the program was killed by a signal.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// The last `n` non-empty lines of stderr (or stdout if stderr is empty), for error messages.
    pub fn tail(&self, n: usize) -> String {
        let source = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        let lines: Vec<&str> = source.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(n)..].join("\n")
    }
}

/// Runs programs. The real one is [`SystemRunner`]; the tests use fakes.
pub trait Runner: Send + Sync {
    /// Runs `program` with `args` and waits for it. `Err` when it could not be started at all.
    fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output>;
    /// Starts `program` and does not wait for it (the new version, after an update).
    fn spawn_detached(&self, program: &Path) -> std::io::Result<()>;
}

/// The facts about the system that the detection looks at.
pub trait Env: Send + Sync {
    fn var(&self, name: &str) -> Option<String>;
    /// The running executable, with symlinks resolved.
    fn current_exe(&self) -> Option<PathBuf>;
    fn is_file(&self, path: &Path) -> bool;
    /// Whether `program` is found in `$PATH`.
    fn has_program(&self, program: &str) -> bool;
    fn in_flatpak(&self) -> bool;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output> {
        let out = Command::new(program).args(args).output()?;
        Ok(Output {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn spawn_detached(&self, program: &Path) -> std::io::Result<()> {
        use std::process::Stdio;
        Command::new(program)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    fn current_exe(&self) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        Some(std::fs::canonicalize(&exe).unwrap_or(exe))
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn has_program(&self, program: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
        })
    }

    fn in_flatpak(&self) -> bool {
        crate::sandbox::in_flatpak()
    }
}

/// Default Homebrew prefix on Linux.
const LINUXBREW: &str = "/home/linuxbrew/.linuxbrew";

/// A package name as the package managers print it; anything else is not trusted.
pub fn valid_package(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._+-".contains(c))
}

/// How doknyu was installed. The first rule that matches wins: Flatpak, AppImage, then where
/// the executable lives (Nix, Homebrew, cargo), then which package owns it (dpkg, rpm, pacman).
/// Runs package managers, so call it off the UI thread.
pub fn detect(env: &dyn Env, run: &dyn Runner) -> InstallKind {
    if env.in_flatpak() {
        return InstallKind::Flatpak;
    }
    if let Some(path) = env.var("APPIMAGE").map(PathBuf::from)
        && env.is_file(&path)
    {
        return InstallKind::AppImage { path };
    }
    let Some(exe) = env.current_exe() else {
        return InstallKind::Unknown {
            exe: PathBuf::new(),
        };
    };
    if exe.starts_with("/nix/store/") {
        return InstallKind::Nix;
    }
    let brew = env
        .var("HOMEBREW_PREFIX")
        .map_or_else(|| PathBuf::from(LINUXBREW), PathBuf::from);
    if exe.starts_with(&brew) || exe.starts_with(LINUXBREW) {
        return InstallKind::Homebrew;
    }
    let cargo_home = env
        .var("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env.var("HOME").map(|h| Path::new(&h).join(".cargo")));
    if cargo_home.is_some_and(|c| exe.parent() == Some(c.join("bin").as_path())) {
        return InstallKind::Cargo;
    }
    let exe_arg = exe.to_string_lossy().into_owned();
    let owner = |program: &str, args: &[&str]| -> Option<String> {
        if !env.has_program(program) {
            return None;
        }
        let mut full: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        full.push(exe_arg.clone());
        let out = run.run(program, &full).ok().filter(Output::ok)?;
        Some(out.stdout)
    };
    // `dpkg-query -S /usr/bin/doknyu` prints `doknyu: /usr/bin/doknyu`.
    if let Some(out) = owner("dpkg-query", &["-S"]) {
        let package = out.lines().next().and_then(|l| l.split(':').next());
        return package_kind(package, |package| InstallKind::Deb { package }, &exe);
    }
    if let Some(out) = owner("rpm", &["-qf", "--qf", "%{NAME}"]) {
        let package = out.lines().next();
        return package_kind(package, |package| InstallKind::Rpm { package }, &exe);
    }
    if let Some(out) = owner("pacman", &["-Qqo"]) {
        let package = out.lines().next();
        return package_kind(package, |package| InstallKind::Pacman { package }, &exe);
    }
    InstallKind::Unknown { exe }
}

/// The package a manager reported, if it is a valid name; otherwise the install is unknown.
fn package_kind(
    package: Option<&str>,
    kind: impl FnOnce(String) -> InstallKind,
    exe: &Path,
) -> InstallKind {
    match package.map(str::trim) {
        Some(p) if valid_package(p) => kind(p.to_owned()),
        _ => InstallKind::Unknown {
            exe: exe.to_path_buf(),
        },
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub struct FakeEnv {
        pub vars: HashMap<String, String>,
        pub exe: Option<PathBuf>,
        pub files: Vec<PathBuf>,
        pub programs: Vec<String>,
        pub flatpak: bool,
    }

    impl Env for FakeEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn current_exe(&self) -> Option<PathBuf> {
            self.exe.clone()
        }
        fn is_file(&self, path: &Path) -> bool {
            self.files.iter().any(|f| f == path)
        }
        fn has_program(&self, program: &str) -> bool {
            self.programs.iter().any(|p| p == program)
        }
        fn in_flatpak(&self) -> bool {
            self.flatpak
        }
    }

    /// Answers each program with a fixed output and records every call.
    #[derive(Default)]
    pub struct FakeRunner {
        pub answers: Mutex<HashMap<String, Vec<Output>>>,
        pub calls: Mutex<Vec<(String, Vec<String>)>>,
        pub spawned: Mutex<Vec<PathBuf>>,
        pub spawn_fails: bool,
    }

    impl FakeRunner {
        /// The next run of `program` answers with `code` and `stdout`; later runs repeat the last
        /// answer given.
        pub fn answer(&self, program: &str, code: i32, stdout: &str) {
            self.answers
                .lock()
                .unwrap()
                .entry(program.to_owned())
                .or_default()
                .push(Output {
                    code,
                    stdout: stdout.to_owned(),
                    stderr: if code == 0 {
                        String::new()
                    } else {
                        format!("{program} failed\nlast line of {program}")
                    },
                });
        }

        pub fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output> {
            self.calls
                .lock()
                .unwrap()
                .push((program.to_owned(), args.to_vec()));
            let mut answers = self.answers.lock().unwrap();
            match answers.get_mut(program) {
                Some(list) if list.len() > 1 => Ok(list.remove(0)),
                Some(list) if !list.is_empty() => Ok(list[0].clone()),
                _ => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            }
        }

        fn spawn_detached(&self, program: &Path) -> std::io::Result<()> {
            if self.spawn_fails {
                return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
            }
            self.spawned.lock().unwrap().push(program.to_path_buf());
            Ok(())
        }
    }

    /// A fresh empty directory under the system temp dir, unique to `name` and this process.
    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("doknyu-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;

    fn env_with_exe(exe: &str) -> FakeEnv {
        FakeEnv {
            exe: Some(PathBuf::from(exe)),
            vars: [("HOME".to_owned(), "/home/u".to_owned())].into(),
            ..FakeEnv::default()
        }
    }

    #[test]
    fn flatpak_wins_over_appimage() {
        let mut env = env_with_exe("/app/bin/doknyu");
        env.flatpak = true;
        env.vars
            .insert("APPIMAGE".into(), "/home/u/doknyu.AppImage".into());
        env.files.push("/home/u/doknyu.AppImage".into());
        assert_eq!(detect(&env, &FakeRunner::default()), InstallKind::Flatpak);
    }

    #[test]
    fn appimage_from_its_variable() {
        let mut env = env_with_exe("/tmp/.mount_x/usr/bin/doknyu");
        env.vars
            .insert("APPIMAGE".into(), "/home/u/doknyu.AppImage".into());
        env.files.push("/home/u/doknyu.AppImage".into());
        assert_eq!(
            detect(&env, &FakeRunner::default()),
            InstallKind::AppImage {
                path: "/home/u/doknyu.AppImage".into()
            }
        );
    }

    #[test]
    fn appimage_variable_to_a_missing_file_is_ignored() {
        let mut env = env_with_exe("/opt/doknyu");
        env.vars
            .insert("APPIMAGE".into(), "/home/u/gone.AppImage".into());
        assert_eq!(
            detect(&env, &FakeRunner::default()),
            InstallKind::Unknown {
                exe: "/opt/doknyu".into()
            }
        );
    }

    #[test]
    fn nix_store() {
        let env = env_with_exe("/nix/store/abc-doknyu-0.1.0/bin/doknyu");
        assert_eq!(detect(&env, &FakeRunner::default()), InstallKind::Nix);
    }

    #[test]
    fn homebrew_default_and_custom_prefix() {
        let env = env_with_exe("/home/linuxbrew/.linuxbrew/Cellar/doknyu/0.1.0/bin/doknyu");
        assert_eq!(detect(&env, &FakeRunner::default()), InstallKind::Homebrew);
        let mut env = env_with_exe("/opt/brew/Cellar/doknyu/0.1.0/bin/doknyu");
        env.vars
            .insert("HOMEBREW_PREFIX".into(), "/opt/brew".into());
        assert_eq!(detect(&env, &FakeRunner::default()), InstallKind::Homebrew);
    }

    #[test]
    fn cargo_bin_dir() {
        let env = env_with_exe("/home/u/.cargo/bin/doknyu");
        assert_eq!(detect(&env, &FakeRunner::default()), InstallKind::Cargo);
        let mut env = env_with_exe("/data/cargo/bin/doknyu");
        env.vars.insert("CARGO_HOME".into(), "/data/cargo".into());
        assert_eq!(detect(&env, &FakeRunner::default()), InstallKind::Cargo);
    }

    #[test]
    fn deb_package_owner() {
        let mut env = env_with_exe("/usr/bin/doknyu");
        env.programs = vec!["dpkg-query".into(), "rpm".into()];
        let run = FakeRunner::default();
        run.answer("dpkg-query", 0, "doknyu: /usr/bin/doknyu\n");
        assert_eq!(
            detect(&env, &run),
            InstallKind::Deb {
                package: "doknyu".into()
            }
        );
        assert_eq!(
            run.calls()[0],
            (
                "dpkg-query".to_owned(),
                vec!["-S".to_owned(), "/usr/bin/doknyu".to_owned()]
            )
        );
    }

    #[test]
    fn dpkg_fails_and_rpm_answers() {
        let mut env = env_with_exe("/usr/bin/doknyu");
        env.programs = vec!["dpkg-query".into(), "rpm".into()];
        let run = FakeRunner::default();
        run.answer("dpkg-query", 1, "");
        run.answer("rpm", 0, "doknyu");
        assert_eq!(
            detect(&env, &run),
            InstallKind::Rpm {
                package: "doknyu".into()
            }
        );
    }

    #[test]
    fn pacman_owner() {
        let mut env = env_with_exe("/usr/bin/doknyu");
        env.programs = vec!["pacman".into()];
        let run = FakeRunner::default();
        run.answer("pacman", 0, "doknyu-bin\n");
        assert_eq!(
            detect(&env, &run),
            InstallKind::Pacman {
                package: "doknyu-bin".into()
            }
        );
    }

    #[test]
    fn invalid_package_name_is_unknown() {
        let mut env = env_with_exe("/usr/bin/doknyu");
        env.programs = vec!["pacman".into()];
        let run = FakeRunner::default();
        run.answer("pacman", 0, "doknyu; rm -rf /\n");
        assert_eq!(
            detect(&env, &run),
            InstallKind::Unknown {
                exe: "/usr/bin/doknyu".into()
            }
        );
    }

    #[test]
    fn nothing_matches_is_unknown() {
        let env = env_with_exe("/opt/doknyu/doknyu");
        assert_eq!(
            detect(&env, &FakeRunner::default()),
            InstallKind::Unknown {
                exe: "/opt/doknyu/doknyu".into()
            }
        );
    }

    #[test]
    fn package_names() {
        for ok in ["doknyu", "doknyu-bin", "lib.x+y-z", "0ad"] {
            assert!(valid_package(ok), "{ok}");
        }
        for bad in ["", "-x", "Doknyu", "a b", "a;b", "a/b"] {
            assert!(!valid_package(bad), "{bad}");
        }
    }

    #[test]
    fn output_tail_prefers_stderr() {
        let out = Output {
            code: 1,
            stdout: "a\nb".into(),
            stderr: "x\n\ny\nz\n".into(),
        };
        assert_eq!(out.tail(2), "y\nz");
        let out = Output {
            code: 1,
            stdout: "a\nb".into(),
            stderr: " ".into(),
        };
        assert_eq!(out.tail(5), "a\nb");
    }
}
