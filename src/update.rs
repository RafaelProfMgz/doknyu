//! "Check for updates": the newest release on GitHub, what updating means for each kind of
//! install ([`plan`]) and doing it. Pure and iced-free; the network is behind [`Http`] and the
//! system behind [`Runner`], so the tests never touch either.
//!
//! The app only talks to the network when the user asks, and only for the installs that need
//! it. A download whose SHA-256 does not match the release's `SHA256SUMS` never replaces the
//! installed app. Known limit: `SHA256SUMS` comes from the same release, so this protects
//! against corrupt or truncated downloads, not against someone able to publish a release.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::install::{InstallKind, Runner};
use crate::uninstall::APP_ID;

/// The version running now.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const LATEST_URL: &str = "https://api.github.com/repos/RafaelProfMgz/doknyu/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/RafaelProfMgz/doknyu/releases/latest";
const APPIMAGE_ASSET: &str = "doknyu-x86_64.AppImage";
const SUMS_ASSET: &str = "SHA256SUMS";

/// `X.Y.Z` (with or without a leading `v`) as three numbers.
pub fn parse_version(s: &str) -> Option<[u64; 3]> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.').map(|p| p.parse::<u64>().ok());
    let v = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(v)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// The parts of a GitHub release the update needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the `v`.
    pub version: String,
    pub html_url: String,
    /// Release notes (Markdown, shown as plain text).
    pub notes: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// Whether this release is newer than the running version.
    pub fn is_newer(&self) -> bool {
        match (parse_version(&self.version), parse_version(CURRENT)) {
            (Some(latest), Some(current)) => latest > current,
            _ => false,
        }
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// No network, DNS or TLS failure.
    Network,
    /// GitHub answered 403/429.
    RateLimited,
    /// Another HTTP status.
    Http(u16),
    /// The release is not what the app expects (tag that is not `X.Y.Z`, bad JSON).
    BadRelease,
    /// The release has no file for this install, or no line for it in `SHA256SUMS`.
    MissingAsset,
    /// The download does not match its checksum; nothing was changed.
    HashMismatch,
    /// The AppImage's folder cannot be written.
    NotWritable(PathBuf),
    /// The user closed the password window (pkexec 126/127).
    Cancelled,
    /// The package manager failed: the last lines it printed.
    Failed(String),
    /// A local file operation failed.
    Io(String),
}

impl From<std::io::Error> for UpdateError {
    fn from(e: std::io::Error) -> Self {
        UpdateError::Io(e.to_string())
    }
}

/// Reads the JSON of `releases/latest`.
pub fn parse_release(json: &str) -> Result<Release, UpdateError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| UpdateError::BadRelease)?;
    let tag = v["tag_name"].as_str().ok_or(UpdateError::BadRelease)?;
    parse_version(tag).ok_or(UpdateError::BadRelease)?;
    let assets = v["assets"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a["name"].as_str()?.to_owned(),
                        url: a["browser_download_url"].as_str()?.to_owned(),
                        size: a["size"].as_u64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version: tag.trim_start_matches('v').to_owned(),
        html_url: v["html_url"].as_str().unwrap_or(RELEASES_URL).to_owned(),
        notes: v["body"].as_str().unwrap_or_default().to_owned(),
        assets,
    })
}

/// The SHA-256 of `name` in a `SHA256SUMS` file (`<hash>  <name>`, or `<hash> *<name>`).
pub fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// How the app installs a downloaded release by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfInstall {
    /// Replaces the AppImage file in place.
    ReplaceAppImage { path: PathBuf },
    /// `pkexec apt-get install -y <file.deb>`.
    AptInstall,
}

/// What updating means for this install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePlan {
    /// Checks GitHub; if there is a newer version, downloads its file and installs it.
    SelfUpdate { install: SelfInstall },
    /// Runs the package manager through pkexec; it decides whether there is something newer.
    Privileged {
        package: String,
        program: &'static str,
        args: Vec<String>,
    },
    /// Only shows the command.
    ShowCommand { command: String },
    /// Checks GitHub and, if there is something newer, offers to open the release page.
    OpenRelease,
}

/// What updating does for `kind`. `has_program` says which programs are in `$PATH`
/// (`pkexec`, `paru`, `yay`).
pub fn plan(kind: &InstallKind, has_program: &dyn Fn(&str) -> bool) -> UpdatePlan {
    let show = |command: String| UpdatePlan::ShowCommand { command };
    match kind {
        InstallKind::AppImage { path } => UpdatePlan::SelfUpdate {
            install: SelfInstall::ReplaceAppImage { path: path.clone() },
        },
        InstallKind::Deb { .. } => UpdatePlan::SelfUpdate {
            install: SelfInstall::AptInstall,
        },
        InstallKind::Rpm { package } if has_program("pkexec") => UpdatePlan::Privileged {
            package: package.clone(),
            program: "pkexec",
            args: ["dnf", "upgrade", "-y", package].map(String::from).to_vec(),
        },
        InstallKind::Rpm { package } => show(format!("sudo dnf upgrade -y {package}")),
        // An AUR helper updates the AUR and the repositories (the [doknyu] pacman repository
        // included); without one, pacman updates from the repository.
        InstallKind::Pacman { .. } => show(if has_program("paru") {
            "paru -Syu".to_owned()
        } else if has_program("yay") {
            "yay -Syu".to_owned()
        } else {
            "sudo pacman -Syu".to_owned()
        }),
        InstallKind::Flatpak => show(format!("flatpak update {APP_ID}")),
        InstallKind::Nix => show("nix profile upgrade doknyu".to_owned()),
        InstallKind::Homebrew => show("brew upgrade doknyu".to_owned()),
        InstallKind::Cargo => show(
            "cargo install --git https://github.com/RafaelProfMgz/doknyu --locked --force"
                .to_owned(),
        ),
        InstallKind::Unknown { .. } => UpdatePlan::OpenRelease,
    }
}

/// The network, as the update uses it.
pub trait Http: Send + Sync {
    /// The body of `url` as text.
    fn get_text(&self, url: &str) -> Result<String, UpdateError>;
    /// Writes the body of `url` into `out`, calling `progress` with the bytes written so far.
    fn download(
        &self,
        url: &str,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), UpdateError>;
}

/// [`Http`] over `ureq` (HTTPS with rustls). Blocking: call it off the UI thread.
pub struct UreqHttp;

fn ureq_error(e: ureq::Error) -> UpdateError {
    match e {
        ureq::Error::StatusCode(403 | 429) => UpdateError::RateLimited,
        ureq::Error::StatusCode(code) => UpdateError::Http(code),
        _ => UpdateError::Network,
    }
}

fn request(url: &str) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    ureq::get(url)
        .header("User-Agent", concat!("doknyu/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
}

impl Http for UreqHttp {
    fn get_text(&self, url: &str) -> Result<String, UpdateError> {
        request(url)
            .call()
            .map_err(ureq_error)?
            .body_mut()
            .read_to_string()
            .map_err(ureq_error)
    }

    fn download(
        &self,
        url: &str,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), UpdateError> {
        let response = request(url).call().map_err(ureq_error)?;
        let mut reader = response
            .into_body()
            .into_with_config()
            .limit(u64::MAX)
            .reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = std::io::Read::read(&mut reader, &mut buf).map_err(|_| UpdateError::Network)?;
            if n == 0 {
                return Ok(());
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(done);
        }
    }
}

/// The newest release on GitHub.
pub fn latest(http: &dyn Http) -> Result<Release, UpdateError> {
    parse_release(&http.get_text(LATEST_URL)?)
}

/// The file of `release` this install needs.
pub fn asset_name(install: &SelfInstall, release: &Release) -> String {
    match install {
        SelfInstall::ReplaceAppImage { .. } => APPIMAGE_ASSET.to_owned(),
        // `cargo deb` names it with the Debian revision `-1`.
        SelfInstall::AptInstall => format!("doknyu_{}-1_amd64.deb", release.version),
    }
}

/// Writes into the file and hashes what goes through.
struct Hashing<'a> {
    file: &'a mut std::fs::File,
    hash: Sha256,
}

impl Write for Hashing<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Downloads `asset` into `dest` (created new) and checks it against `expected`. On any
/// failure `dest` is removed.
fn download_checked(
    http: &dyn Http,
    asset: &Asset,
    expected: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), UpdateError> {
    let result = (|| {
        let mut file = std::fs::File::create(dest)?;
        let mut out = Hashing {
            file: &mut file,
            hash: Sha256::new(),
        };
        let total = asset.size;
        http.download(&asset.url, &mut out, &mut |done| progress(done, total))?;
        out.flush()?;
        let got: String = out
            .hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        file.sync_all()?;
        if got == expected {
            Ok(())
        } else {
            Err(UpdateError::HashMismatch)
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

/// Downloads the file of `release` for `install`, checks it and installs it. `cache` is where
/// a `.deb` waits for apt (`~/.cache/doknyu`). `progress` gets (bytes done, bytes total).
/// Blocking: run it off the UI thread.
pub fn self_update(
    release: &Release,
    install: &SelfInstall,
    http: &dyn Http,
    runner: &dyn Runner,
    cache: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), UpdateError> {
    let name = asset_name(install, release);
    let asset = release.asset(&name).ok_or(UpdateError::MissingAsset)?;
    match install {
        SelfInstall::ReplaceAppImage { path } => {
            // Next to the AppImage, so the final rename stays on one file system (atomic). If
            // the folder cannot be written, fail before downloading anything.
            let dir = path.parent().unwrap_or(Path::new("."));
            let temp = dir.join(format!(".doknyu-update-{}.AppImage", std::process::id()));
            std::fs::File::create(&temp)
                .map_err(|_| UpdateError::NotWritable(dir.to_path_buf()))?;
            let expected = match sums_line(http, release, &name) {
                Ok(hash) => hash,
                Err(e) => {
                    let _ = std::fs::remove_file(&temp);
                    return Err(e);
                }
            };
            download_checked(http, asset, &expected, &temp, progress)?;
            let finish = || -> std::io::Result<()> {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
                std::fs::rename(&temp, path)
            };
            finish().map_err(|e| {
                let _ = std::fs::remove_file(&temp);
                UpdateError::from(e)
            })
        }
        SelfInstall::AptInstall => {
            let expected = sums_line(http, release, &name)?;
            std::fs::create_dir_all(cache)?;
            let deb = cache.join(&name);
            download_checked(http, asset, &expected, &deb, progress)?;
            let args = vec![
                "apt-get".to_owned(),
                "install".to_owned(),
                "-y".to_owned(),
                deb.to_string_lossy().into_owned(),
            ];
            let result = run_privileged(runner, &args);
            let _ = std::fs::remove_file(&deb);
            result
        }
    }
}

/// The checksum of `name` from the release's `SHA256SUMS`.
fn sums_line(http: &dyn Http, release: &Release, name: &str) -> Result<String, UpdateError> {
    let sums = release.asset(SUMS_ASSET).ok_or(UpdateError::MissingAsset)?;
    checksum_for(&http.get_text(&sums.url)?, name).ok_or(UpdateError::MissingAsset)
}

/// Runs `pkexec <args>` and turns its exit code into a result.
fn run_privileged(runner: &dyn Runner, args: &[String]) -> Result<(), UpdateError> {
    let out = runner
        .run("pkexec", args)
        .map_err(|e| UpdateError::Failed(e.to_string()))?;
    match out.code {
        0 => Ok(()),
        126 | 127 => Err(UpdateError::Cancelled),
        _ => Err(UpdateError::Failed(out.tail(5))),
    }
}

/// Runs a [`UpdatePlan::Privileged`] upgrade. `Ok(true)` when the package version changed.
pub fn privileged_upgrade(
    runner: &dyn Runner,
    package: &str,
    args: &[String],
) -> Result<bool, UpdateError> {
    let version = || {
        runner
            .run(
                "rpm",
                &["-q", "--qf", "%{VERSION}", package].map(String::from),
            )
            .ok()
            .filter(|o| o.ok())
            .map(|o| o.stdout.trim().to_owned())
    };
    let before = version();
    run_privileged(runner, args)?;
    Ok(version() != before)
}

/// `~/.cache/doknyu` (or `$XDG_CACHE_HOME/doknyu`).
pub fn cache_dir() -> PathBuf {
    let xdg = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    xdg.unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
    })
    .join("doknyu")
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// Serves fixed bodies by URL and records which URLs were asked for.
    #[derive(Default)]
    pub struct FakeHttp {
        pub bodies: HashMap<String, Result<Vec<u8>, UpdateError>>,
        pub asked: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        pub fn with(mut self, url: &str, body: &[u8]) -> Self {
            self.bodies.insert(url.to_owned(), Ok(body.to_vec()));
            self
        }

        pub fn failing(mut self, url: &str, e: UpdateError) -> Self {
            self.bodies.insert(url.to_owned(), Err(e));
            self
        }

        fn body(&self, url: &str) -> Result<Vec<u8>, UpdateError> {
            self.asked.lock().unwrap().push(url.to_owned());
            self.bodies
                .get(url)
                .cloned()
                .unwrap_or(Err(UpdateError::Http(404)))
        }
    }

    impl Http for FakeHttp {
        fn get_text(&self, url: &str) -> Result<String, UpdateError> {
            self.body(url)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        }

        fn download(
            &self,
            url: &str,
            out: &mut dyn Write,
            progress: &mut dyn FnMut(u64),
        ) -> Result<(), UpdateError> {
            let body = self.body(url)?;
            for chunk in body.chunks(4) {
                out.write_all(chunk)?;
            }
            progress(body.len() as u64);
            Ok(())
        }
    }

    pub fn sha256_hex(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A release `version` with the AppImage, the `.deb` and `SHA256SUMS` for these bodies, and
    /// the `FakeHttp` serving them (plus the `latest` JSON).
    pub fn release_with(version: &str, appimage: &[u8], deb: &[u8]) -> (Release, FakeHttp) {
        let deb_name = format!("doknyu_{version}-1_amd64.deb");
        let sums = format!(
            "{}  doknyu-x86_64.AppImage\n{}  {deb_name}\n",
            sha256_hex(appimage),
            sha256_hex(deb)
        );
        let asset = |name: &str, size: usize| Asset {
            name: name.to_owned(),
            url: format!("https://dl/{name}"),
            size: size as u64,
        };
        let release = Release {
            version: version.to_owned(),
            html_url: format!("https://github.com/RafaelProfMgz/doknyu/releases/tag/v{version}"),
            notes: "- Images screen".to_owned(),
            assets: vec![
                asset("doknyu-x86_64.AppImage", appimage.len()),
                asset(&deb_name, deb.len()),
                asset("SHA256SUMS", sums.len()),
            ],
        };
        let json = serde_json::json!({
            "tag_name": format!("v{version}"),
            "html_url": release.html_url,
            "body": release.notes,
            "assets": release.assets.iter().map(|a| serde_json::json!({
                "name": a.name, "browser_download_url": a.url, "size": a.size,
            })).collect::<Vec<_>>(),
        });
        let http = FakeHttp::default()
            .with(LATEST_URL, json.to_string().as_bytes())
            .with("https://dl/doknyu-x86_64.AppImage", appimage)
            .with(&format!("https://dl/{deb_name}"), deb)
            .with("https://dl/SHA256SUMS", sums.as_bytes());
        (release, http)
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;
    use crate::install::fakes::{FakeRunner, temp_dir};

    /// Real network: the latest release on GitHub parses and has the files the update needs.
    #[test]
    #[ignore = "talks to GitHub"]
    fn real_latest_release() {
        let release = latest(&UreqHttp).unwrap();
        assert!(parse_version(&release.version).is_some());
        assert!(release.asset("SHA256SUMS").is_some());
        assert!(release.asset(APPIMAGE_ASSET).is_some());
        let sums = UreqHttp
            .get_text(&release.asset("SHA256SUMS").unwrap().url)
            .unwrap();
        assert!(checksum_for(&sums, APPIMAGE_ASSET).is_some());
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("0.1.0"), Some([0, 1, 0]));
        assert_eq!(parse_version("v1.20.3"), Some([1, 20, 3]));
        assert!(parse_version("0.10.0") > parse_version("0.9.0"));
        assert!(parse_version("0.2.0") > parse_version("0.1.0"));
        for bad in ["", "1.2", "1.2.3.4", "v1.x.0", "1.2.3-rc1"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn newer_than_current() {
        let at = |v: &str| Release {
            version: v.into(),
            html_url: String::new(),
            notes: String::new(),
            assets: vec![],
        };
        assert!(!at(CURRENT).is_newer());
        assert!(at("999.0.0").is_newer());
        assert!(!at("0.0.1").is_newer());
    }

    #[test]
    fn release_json() {
        let (release, http) = release_with("0.2.0", b"img", b"deb");
        assert_eq!(latest(&http), Ok(release));
        assert_eq!(parse_release("{}"), Err(UpdateError::BadRelease));
        assert_eq!(
            parse_release(r#"{"tag_name":"nightly"}"#),
            Err(UpdateError::BadRelease)
        );
        assert_eq!(parse_release("not json"), Err(UpdateError::BadRelease));
    }

    #[test]
    fn checksum_lines() {
        let h = "a".repeat(64);
        let sums = format!("{h}  doknyu-x86_64.AppImage\n{}  other\n", "b".repeat(64));
        assert_eq!(
            checksum_for(&sums, "doknyu-x86_64.AppImage"),
            Some(h.clone())
        );
        assert_eq!(
            checksum_for(&format!("{h} *x.deb"), "x.deb"),
            Some(h.clone())
        );
        assert_eq!(checksum_for(&sums, "missing"), None);
        assert_eq!(checksum_for("short  x.deb", "x.deb"), None);
    }

    #[test]
    fn plans() {
        let none = |_: &str| false;
        let all = |_: &str| true;
        let only = |p: &'static str| move |q: &str| q == p;
        assert_eq!(
            plan(&InstallKind::AppImage { path: "/a".into() }, &none),
            UpdatePlan::SelfUpdate {
                install: SelfInstall::ReplaceAppImage { path: "/a".into() }
            }
        );
        assert_eq!(
            plan(
                &InstallKind::Deb {
                    package: "doknyu".into()
                },
                &none
            ),
            UpdatePlan::SelfUpdate {
                install: SelfInstall::AptInstall
            }
        );
        let rpm = InstallKind::Rpm {
            package: "doknyu".into(),
        };
        assert_eq!(
            plan(&rpm, &all),
            UpdatePlan::Privileged {
                package: "doknyu".into(),
                program: "pkexec",
                args: ["dnf", "upgrade", "-y", "doknyu"]
                    .map(String::from)
                    .to_vec(),
            }
        );
        assert_eq!(
            plan(&rpm, &none),
            UpdatePlan::ShowCommand {
                command: "sudo dnf upgrade -y doknyu".into()
            }
        );
        let aur = InstallKind::Pacman {
            package: "doknyu-bin".into(),
        };
        let cmd = |p: UpdatePlan| match p {
            UpdatePlan::ShowCommand { command } => command,
            other => panic!("{other:?}"),
        };
        assert_eq!(cmd(plan(&aur, &all)), "paru -Syu");
        assert_eq!(cmd(plan(&aur, &only("yay"))), "yay -Syu");
        assert_eq!(cmd(plan(&aur, &none)), "sudo pacman -Syu");
        assert_eq!(
            cmd(plan(&InstallKind::Flatpak, &none)),
            "flatpak update io.github.rafaelprofmgz.Doknyu"
        );
        assert_eq!(
            cmd(plan(&InstallKind::Homebrew, &none)),
            "brew upgrade doknyu"
        );
        assert_eq!(
            cmd(plan(&InstallKind::Nix, &none)),
            "nix profile upgrade doknyu"
        );
        assert!(cmd(plan(&InstallKind::Cargo, &none)).starts_with("cargo install --git"));
        assert_eq!(
            plan(&InstallKind::Unknown { exe: "/x".into() }, &none),
            UpdatePlan::OpenRelease
        );
    }

    #[test]
    fn appimage_is_replaced_and_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("update-appimage");
        let path = dir.join("doknyu.AppImage");
        std::fs::write(&path, b"old").unwrap();
        let (release, http) = release_with("9.0.0", b"new appimage", b"deb");
        let install = SelfInstall::ReplaceAppImage { path: path.clone() };
        let mut seen = Vec::new();
        self_update(
            &release,
            &install,
            &http,
            &FakeRunner::default(),
            &dir,
            &mut |done, total| seen.push((done, total)),
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new appimage");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(seen.last(), Some(&(12, 12)));
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wrong_hash_leaves_the_appimage_untouched() {
        let dir = temp_dir("update-badhash");
        let path = dir.join("doknyu.AppImage");
        std::fs::write(&path, b"old").unwrap();
        let (release, http) = release_with("9.0.0", b"new appimage", b"deb");
        let http = http.with("https://dl/doknyu-x86_64.AppImage", b"tampered");
        let install = SelfInstall::ReplaceAppImage { path: path.clone() };
        let r = self_update(
            &release,
            &install,
            &http,
            &FakeRunner::default(),
            &dir,
            &mut |_, _| {},
        );
        assert_eq!(r, Err(UpdateError::HashMismatch));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "temp file removed"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unwritable_folder_fails_before_downloading() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("update-ro");
        let path = dir.join("doknyu.AppImage");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let (release, http) = release_with("9.0.0", b"new", b"deb");
        let install = SelfInstall::ReplaceAppImage { path: path.clone() };
        let r = self_update(
            &release,
            &install,
            &http,
            &FakeRunner::default(),
            &dir,
            &mut |_, _| {},
        );
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Root can write anywhere: then the check does not apply.
        if r.is_err() {
            assert_eq!(r, Err(UpdateError::NotWritable(dir.clone())));
            assert!(http.asked.lock().unwrap().is_empty());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_asset_or_checksum() {
        let dir = temp_dir("update-missing");
        let (mut release, http) = release_with("9.0.0", b"img", b"deb");
        let install = SelfInstall::AptInstall;
        let http = http.with("https://dl/SHA256SUMS", b"");
        let r = self_update(
            &release,
            &install,
            &http,
            &FakeRunner::default(),
            &dir,
            &mut |_, _| {},
        );
        assert_eq!(r, Err(UpdateError::MissingAsset));
        release.assets.retain(|a| a.name == "SHA256SUMS");
        let r = self_update(
            &release,
            &install,
            &http,
            &FakeRunner::default(),
            &dir,
            &mut |_, _| {},
        );
        assert_eq!(r, Err(UpdateError::MissingAsset));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deb_goes_through_apt_and_the_file_is_removed() {
        for (code, expected) in [
            (0, Ok(())),
            (126, Err(UpdateError::Cancelled)),
            (
                1,
                Err(UpdateError::Failed(
                    "pkexec failed\nlast line of pkexec".into(),
                )),
            ),
        ] {
            let dir = temp_dir(&format!("update-deb-{code}"));
            let (release, http) = release_with("9.0.0", b"img", b"the deb");
            let run = FakeRunner::default();
            run.answer("pkexec", code, "");
            let r = self_update(
                &release,
                &SelfInstall::AptInstall,
                &http,
                &run,
                &dir,
                &mut |_, _| {},
            );
            assert_eq!(r, expected);
            let deb = dir.join("doknyu_9.0.0-1_amd64.deb");
            assert_eq!(
                run.calls(),
                vec![(
                    "pkexec".to_owned(),
                    vec![
                        "apt-get".to_owned(),
                        "install".to_owned(),
                        "-y".to_owned(),
                        deb.to_string_lossy().into_owned()
                    ]
                )]
            );
            assert!(!deb.exists());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn dnf_upgrade_reports_whether_it_changed() {
        let args = ["dnf", "upgrade", "-y", "doknyu"].map(String::from);
        let run = FakeRunner::default();
        run.answer("rpm", 0, "0.1.0");
        run.answer("rpm", 0, "0.2.0");
        run.answer("pkexec", 0, "");
        assert_eq!(privileged_upgrade(&run, "doknyu", &args), Ok(true));
        let run = FakeRunner::default();
        run.answer("rpm", 0, "0.1.0");
        run.answer("pkexec", 0, "");
        assert_eq!(privileged_upgrade(&run, "doknyu", &args), Ok(false));
        let run = FakeRunner::default();
        run.answer("rpm", 0, "0.1.0");
        run.answer("pkexec", 126, "");
        assert_eq!(
            privileged_upgrade(&run, "doknyu", &args),
            Err(UpdateError::Cancelled)
        );
    }
}
