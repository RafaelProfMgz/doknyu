use std::io::ErrorKind;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DockerError {
    #[error("Docker is not installed")]
    NotInstalled,
    #[error("Docker daemon is not running")]
    NotRunning,
    #[error("Permission denied accessing the Docker socket")]
    PermissionDenied,
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Docker API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("{0}")]
    Other(String),
}

impl DockerError {
    /// True for errors that mean the daemon cannot be reached.
    pub fn is_connection(&self) -> bool {
        matches!(
            self,
            DockerError::NotInstalled | DockerError::NotRunning | DockerError::PermissionDenied
        )
    }
}

/// Maps an I/O error from connecting to the Docker socket to a `DockerError`.
pub fn classify_connect_error(
    kind: ErrorKind,
    socket_exists: bool,
    cli_installed: bool,
) -> DockerError {
    match kind {
        ErrorKind::PermissionDenied => DockerError::PermissionDenied,
        ErrorKind::NotFound if !socket_exists && !cli_installed => DockerError::NotInstalled,
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => DockerError::NotRunning,
        other => DockerError::Other(format!("connection error: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContainerState;
    use std::io::ErrorKind::*;

    #[test]
    fn classifies_connect_errors() {
        assert_eq!(
            classify_connect_error(PermissionDenied, true, true),
            DockerError::PermissionDenied
        );
        assert_eq!(
            classify_connect_error(NotFound, false, false),
            DockerError::NotInstalled
        );
        assert_eq!(
            classify_connect_error(NotFound, false, true),
            DockerError::NotRunning
        );
        assert_eq!(
            classify_connect_error(ConnectionRefused, true, true),
            DockerError::NotRunning
        );
        assert!(matches!(
            classify_connect_error(TimedOut, true, true),
            DockerError::Other(_)
        ));
    }

    #[test]
    fn container_state_error_flags() {
        assert!(ContainerState::Exited { code: 137 }.is_error());
        assert!(!ContainerState::Exited { code: 0 }.is_error());
        assert!(ContainerState::Dead.is_error());
        assert!(ContainerState::Running.is_running());
        assert!(!ContainerState::Paused.is_running());
    }

    #[test]
    fn connection_errors_are_flagged() {
        assert!(DockerError::NotRunning.is_connection());
        assert!(!DockerError::NotFound("x".into()).is_connection());
    }
}
