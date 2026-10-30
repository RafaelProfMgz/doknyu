//! The built-in exec bridge: `doknyu --exec-bridge <container>` connects the terminal it runs in
//! to a shell in the container through the Docker API, as `docker exec -it` does. The Terminal
//! tab (a PTY of `iced_term`, see `docs/decisions/0001-terminal.md`) and the external terminal
//! run it, so neither needs the `docker` CLI (it is not in the Flatpak sandbox).

use std::sync::Arc;

use futures::StreamExt;
use rustix::termios::{self, OptionalActions, Termios};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::docker::{DockerClient, ExecControl, ExecSession};
use crate::error::DockerError;

/// The flag that makes the binary run as the bridge.
pub const FLAG: &str = "--exec-bridge";

/// The container to bridge to, from the process arguments (`args[0]` is the program).
/// `Ok(None)` is a normal start of the app; `Err` is the usage message.
pub fn bridge_target(args: &[String]) -> Result<Option<String>, String> {
    if args.get(1).map(String::as_str) != Some(FLAG) {
        return Ok(None);
    }
    match args.get(2) {
        Some(id) if !id.is_empty() => Ok(Some(id.clone())),
        _ => Err(format!("usage: doknyu {FLAG} <container>")),
    }
}

/// The arguments after the program that run the bridge for `container_id`.
pub fn bridge_args(container_id: &str) -> Vec<String> {
    vec![FLAG.to_owned(), container_id.to_owned()]
}

/// This executable, which is what the terminals run (`doknyu` from `PATH` if it is unknown).
pub fn program() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "doknyu".to_owned())
}

/// The command run in the container: bash if there is one, else sh.
pub fn shell_command() -> Vec<String> {
    [
        "sh",
        "-c",
        "command -v bash >/dev/null && exec bash || exec sh",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The environment of that command (an app started from a launcher has no `TERM` to pass on).
pub fn shell_env() -> Vec<String> {
    ["TERM=xterm-256color", "COLORTERM=truecolor"]
        .map(str::to_owned)
        .to_vec()
}

/// Puts stdin in raw mode and restores it on drop, also when the bridge ends in an error.
struct RawMode(Termios);

impl RawMode {
    /// `None` when stdin is not a terminal (a pipe, in the tests).
    fn enter() -> Option<Self> {
        let stdin = rustix::stdio::stdin();
        if !termios::isatty(stdin) {
            return None;
        }
        let saved = termios::tcgetattr(stdin).ok()?;
        let mut raw = saved.clone();
        raw.make_raw();
        termios::tcsetattr(stdin, OptionalActions::Now, &raw).ok()?;
        Some(Self(saved))
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(rustix::stdio::stdin(), OptionalActions::Now, &self.0);
    }
}

/// Size of the terminal on stdin as `(columns, rows)`.
fn window_size() -> Option<(u16, u16)> {
    let size = termios::tcgetwinsize(rustix::stdio::stdin()).ok()?;
    (size.ws_col > 0 && size.ws_row > 0).then_some((size.ws_col, size.ws_row))
}

async fn send_size(control: &dyn ExecControl) {
    if let Some((cols, rows)) = window_size()
        && let Err(e) = control.resize(cols, rows).await
    {
        tracing::debug!("could not resize the exec: {e}");
    }
}

/// Resolves on each SIGWINCH, never without a terminal.
async fn window_changed(signal: &mut Option<tokio::signal::unix::Signal>) {
    match signal {
        Some(signal) => {
            signal.recv().await;
        }
        None => std::future::pending().await,
    }
}

/// Runs a shell in `container` until it ends and returns its exit code.
pub async fn run(client: Arc<dyn DockerClient>, container: &str) -> Result<i32, DockerError> {
    let ExecSession {
        mut output,
        mut input,
        control,
    } = client
        .exec_interactive(container, shell_command(), shell_env())
        .await?;

    let raw = RawMode::enter();
    let mut winch = match raw {
        Some(_) => {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change()).ok()
        }
        None => None,
    };
    if raw.is_some() {
        send_size(control.as_ref()).await;
    }

    // stdin -> exec. A blocking read of stdin cannot be cancelled, so this task is left to die
    // with the process.
    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = input.shutdown().await;
                    break;
                }
                Ok(n) => {
                    if input.write_all(&buf[..n]).await.is_err() || input.flush().await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // exec -> stdout, until the output ends.
    let mut stdout = tokio::io::stdout();
    let result: Result<(), DockerError> = loop {
        tokio::select! {
            chunk = output.next() => match chunk {
                None => break Ok(()),
                Some(Err(e)) => break Err(e),
                Some(Ok(bytes)) => {
                    if stdout.write_all(&bytes).await.is_err() || stdout.flush().await.is_err() {
                        break Ok(());
                    }
                }
            },
            () = window_changed(&mut winch) => send_size(control.as_ref()).await,
        }
    };
    drop(raw);
    result?;
    let code = control.exit_code().await?;
    Ok(code.map_or(0, |c| i32::try_from(c).unwrap_or(1)))
}

/// The bridge as a process: connects like the app does, runs, and returns the exit code
/// (1 with a message on stderr when it cannot).
pub fn main(container: &str) -> i32 {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("doknyu: {e}");
            return 1;
        }
    };
    let code = runtime.block_on(async {
        match run(crate::docker::connect(), container).await {
            Ok(code) => code,
            Err(e) => {
                eprintln!("doknyu: {e}");
                1
            }
        }
    });
    // The task reading stdin may be blocked in `read`: do not wait for it.
    runtime.shutdown_background();
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_exec_bridge_args() {
        assert_eq!(
            bridge_target(&args(&["doknyu", "--exec-bridge", "web"])),
            Ok(Some("web".to_string()))
        );
        assert_eq!(bridge_target(&args(&["doknyu"])), Ok(None));
        assert_eq!(bridge_target(&args(&["doknyu", "--other"])), Ok(None));
        assert!(bridge_target(&args(&["doknyu", "--exec-bridge"])).is_err());
        assert!(bridge_target(&args(&["doknyu", "--exec-bridge", ""])).is_err());
    }

    #[test]
    fn command_args_hold_the_flag_and_the_container() {
        assert_eq!(bridge_args("web-id"), ["--exec-bridge", "web-id"]);
    }

    #[test]
    fn shell_prefers_bash_and_term_is_set() {
        let cmd = shell_command();
        assert_eq!(cmd[..2], ["sh", "-c"]);
        assert_eq!(cmd[2], "command -v bash >/dev/null && exec bash || exec sh");
        assert_eq!(shell_env(), ["TERM=xterm-256color", "COLORTERM=truecolor"]);
    }
}
