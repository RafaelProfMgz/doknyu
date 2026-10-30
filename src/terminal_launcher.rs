//! Opens a shell inside a container in the system terminal emulator. Pure and iced-free; the
//! shell is the exec bridge (`crate::exec_bridge`), the same one the embedded Terminal tab runs.

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
fn run_prefix(bin: &str, own_args: &[&str]) -> Vec<String> {
    let base = bin.rsplit('/').next().unwrap_or(bin);
    let flags: &[&str] = match base {
        "gnome-terminal" => &["--"],
        "wezterm" => &["start", "--"],
        // `-e` takes a single string there; `-x` takes the rest of the arguments.
        "xfce4-terminal" => &["-x"],
        _ => &["-e"],
    };
    std::iter::once(bin)
        .chain(own_args.iter().copied())
        .chain(flags.iter().copied())
        .map(str::to_owned)
        .collect()
}

/// The terminal to use and the arguments that precede the command to run in it. `available`
/// says whether a binary can be run (found on `PATH`). `$TERMINAL` may carry arguments
/// (`kitty --single-instance`): its first word is the program and the rest go before the flag
/// that runs a command.
pub fn detect_terminal(
    env_terminal: Option<&str>,
    available: &dyn Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let env_words: Vec<&str> = env_terminal
        .map(|value| value.split_whitespace().collect())
        .unwrap_or_default();
    if let Some((bin, args)) = env_words.split_first()
        && available(bin)
    {
        return Some(run_prefix(bin, args));
    }
    CANDIDATES
        .into_iter()
        .find(|bin| available(bin))
        .map(|bin| run_prefix(bin, &[]))
}

/// The full command line: the terminal's `prefix`, then this executable as the exec bridge.
pub fn command_line(prefix: &[String], exe: &str, container_id: &str) -> Vec<String> {
    prefix
        .iter()
        .cloned()
        .chain(std::iter::once(exe.to_owned()))
        .chain(crate::exec_bridge::bridge_args(container_id))
        .collect()
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

/// Opens a system terminal running a shell in the container, without waiting for it.
pub fn launch(container_id: &str) -> Result<(), DockerError> {
    use std::process::{Command, Stdio};

    let env_terminal = std::env::var("TERMINAL").ok();
    let Some(prefix) = detect_terminal(env_terminal.as_deref(), &on_path) else {
        return Err(DockerError::Other(
            "no terminal emulator found (set $TERMINAL)".into(),
        ));
    };
    let line = command_line(&prefix, &crate::exec_bridge::program(), container_id);
    let mut child = Command::new(&line[0])
        .args(&line[1..])
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
    fn xfce4_terminal_takes_an_argv_with_x() {
        // `-e` takes one string for xfce4-terminal; `-x` takes the rest of the arguments.
        let found = detect_terminal(None, &only(&["xfce4-terminal"]));
        assert_eq!(
            found,
            Some(vec!["xfce4-terminal".to_string(), "-x".to_string()])
        );
    }

    #[test]
    fn env_terminal_keeps_its_arguments_before_the_run_flag() {
        let found = detect_terminal(Some("kitty --single-instance"), &only(&["kitty", "xterm"]));
        assert_eq!(
            found,
            Some(vec![
                "kitty".to_string(),
                "--single-instance".to_string(),
                "-e".to_string()
            ])
        );
        // The run flag is the one of the program, not of its arguments.
        let found = detect_terminal(
            Some("/usr/bin/gnome-terminal --window --maximize"),
            &only(&["/usr/bin/gnome-terminal"]),
        );
        assert_eq!(
            found,
            Some(vec![
                "/usr/bin/gnome-terminal".to_string(),
                "--window".to_string(),
                "--maximize".to_string(),
                "--".to_string()
            ])
        );
        // Only the program is looked up in PATH, and a blank value is no terminal.
        assert_eq!(
            detect_terminal(Some("nope --x"), &only(&["xterm"])),
            Some(vec!["xterm".to_string(), "-e".to_string()])
        );
        assert_eq!(
            detect_terminal(Some("   "), &only(&["xterm"])),
            Some(vec!["xterm".to_string(), "-e".to_string()])
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
    fn launcher_runs_the_bridge() {
        let prefix = vec!["kitty".to_string(), "-e".to_string()];
        assert_eq!(
            command_line(&prefix, "/opt/doknyu", "abc123"),
            ["kitty", "-e", "/opt/doknyu", "--exec-bridge", "abc123"]
        );
    }
}
