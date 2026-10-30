//! Opens a shell inside a container in the system terminal emulator. Pure and iced-free; the
//! embedded Terminal tab (phase 5.2) reuses `exec_args` and `docker_cli_available`.

use crate::error::DockerError;

/// Terminals tried, in order, after `$TERMINAL`.
const CANDIDATES: [&str; 9] = [
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "xfce4-terminal",
    "xterm",
];

/// The command prefix that makes `bin` run what follows it.
fn run_prefix(bin: &str) -> Vec<String> {
    let base = bin.rsplit('/').next().unwrap_or(bin);
    let flags: &[&str] = match base {
        "gnome-terminal" => &["--"],
        "wezterm" => &["start", "--"],
        _ => &["-e"],
    };
    std::iter::once(bin)
        .chain(flags.iter().copied())
        .map(str::to_owned)
        .collect()
}

/// The terminal to use and the arguments that precede the command to run in it. `available`
/// says whether a binary can be run (found on `PATH`).
pub fn detect_terminal(
    env_terminal: Option<&str>,
    available: &dyn Fn(&str) -> bool,
) -> Option<Vec<String>> {
    env_terminal
        .map(str::trim)
        .filter(|bin| !bin.is_empty())
        .into_iter()
        .chain(CANDIDATES)
        .find(|bin| available(bin))
        .map(run_prefix)
}

/// The arguments after `docker` that open a shell in the container.
pub fn exec_args(container_id: &str) -> Vec<String> {
    [
        "exec",
        "-it",
        "-e",
        "TERM=xterm-256color",
        container_id,
        "sh",
        "-c",
        "command -v bash >/dev/null && exec bash || exec sh",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Whether `bin` is an executable file in a `PATH` directory (or a path that is one).
fn on_path(bin: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let is_exec = |p: &std::path::Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if bin.contains('/') {
        return is_exec(std::path::Path::new(bin));
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| is_exec(&dir.join(bin))))
}

/// Whether the `docker` CLI is on `PATH`.
pub fn docker_cli_available() -> bool {
    on_path("docker")
}

/// Opens a system terminal running a shell in the container, without waiting for it.
pub fn launch(container_id: &str) -> Result<(), DockerError> {
    use std::process::{Command, Stdio};

    if !docker_cli_available() {
        return Err(DockerError::Other(
            "the docker command was not found in PATH".into(),
        ));
    }
    let env_terminal = std::env::var("TERMINAL").ok();
    let Some(prefix) = detect_terminal(env_terminal.as_deref(), &on_path) else {
        return Err(DockerError::Other(
            "no terminal emulator found (set $TERMINAL)".into(),
        ));
    };
    let mut child = Command::new(&prefix[0])
        .args(&prefix[1..])
        .arg("docker")
        .args(exec_args(container_id))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| DockerError::Other(format!("could not start {}: {e}", prefix[0])))?;
    // Reap the child when the terminal closes so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only<'a>(names: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |bin| names.contains(&bin)
    }

    #[test]
    fn env_terminal_wins() {
        let found = detect_terminal(Some("kitty"), &only(&["kitty", "gnome-terminal", "xterm"]));
        assert_eq!(found, Some(vec!["kitty".to_string(), "-e".to_string()]));
    }

    #[test]
    fn env_terminal_missing_from_path_is_skipped() {
        let found = detect_terminal(Some("nope"), &only(&["xterm"]));
        assert_eq!(found, Some(vec!["xterm".to_string(), "-e".to_string()]));
        let found = detect_terminal(Some(""), &only(&["xterm"]));
        assert_eq!(found, Some(vec!["xterm".to_string(), "-e".to_string()]));
    }

    #[test]
    fn gnome_terminal_uses_double_dash() {
        let found = detect_terminal(None, &only(&["gnome-terminal", "xterm"]));
        assert_eq!(
            found,
            Some(vec!["gnome-terminal".to_string(), "--".to_string()])
        );
    }

    #[test]
    fn wezterm_uses_start() {
        let found = detect_terminal(None, &only(&["wezterm"]));
        assert_eq!(
            found,
            Some(vec![
                "wezterm".to_string(),
                "start".to_string(),
                "--".to_string()
            ])
        );
    }

    #[test]
    fn x_terminal_emulator_comes_before_gnome_terminal() {
        let found = detect_terminal(None, &only(&["gnome-terminal", "x-terminal-emulator"]));
        assert_eq!(
            found,
            Some(vec!["x-terminal-emulator".to_string(), "-e".to_string()])
        );
    }

    #[test]
    fn falls_back_to_xterm() {
        let found = detect_terminal(None, &only(&["xterm"]));
        assert_eq!(found, Some(vec!["xterm".to_string(), "-e".to_string()]));
    }

    #[test]
    fn none_available_returns_none() {
        assert_eq!(detect_terminal(Some("kitty"), &only(&[])), None);
    }

    #[test]
    fn exec_args_runs_a_shell_with_term_set() {
        let args = exec_args("abc123");
        assert_eq!(
            &args[..6],
            ["exec", "-it", "-e", "TERM=xterm-256color", "abc123", "sh"]
        );
        assert_eq!(args[6], "-c");
        assert_eq!(
            args[7],
            "command -v bash >/dev/null && exec bash || exec sh"
        );
        assert_eq!(args.len(), 8);
    }
}
