pub mod bollard;
pub mod convert;
#[cfg(any(test, feature = "fake"))]
pub mod fake;

use std::sync::Arc;

use crate::error::DockerError;
use crate::model::*;

pub use self::bollard::BollardClient;
#[cfg(any(test, feature = "fake"))]
pub use self::fake::FakeClient;

pub type DockerStream<T> = futures::stream::BoxStream<'static, Result<T, DockerError>>;

/// What an interactive exec needs once it runs: the terminal-side handles of the bridge.
#[async_trait::async_trait]
pub trait ExecControl: Send + Sync {
    /// Tells the exec's PTY the size of the terminal in front of it.
    async fn resize(&self, cols: u16, rows: u16) -> Result<(), DockerError>;
    /// The exit code of the command once it ended; `None` if the daemon does not report one.
    async fn exit_code(&self) -> Result<Option<i64>, DockerError>;
}

/// An exec with a TTY and stdin attached (`docker exec -it`), as the exec bridge uses it.
/// `output` is the raw bytes of the PTY (stdout and stderr are merged by the TTY).
pub struct ExecSession {
    pub output: DockerStream<Vec<u8>>,
    pub input: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    pub control: Box<dyn ExecControl>,
}

#[async_trait::async_trait]
pub trait DockerClient: Send + Sync + 'static {
    async fn ping(&self) -> Result<(), DockerError>;
    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>, DockerError>;
    async fn start(&self, id: &str) -> Result<(), DockerError>;
    async fn stop(&self, id: &str) -> Result<(), DockerError>;
    async fn restart(&self, id: &str) -> Result<(), DockerError>;
    async fn remove(&self, id: &str, force: bool) -> Result<(), DockerError>;
    fn logs(&self, id: &str, tail: usize) -> DockerStream<LogLine>;
    fn stats(&self, id: &str) -> DockerStream<RawStats>;
    fn events(&self) -> DockerStream<DockerEvent>;
    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError>;
    async fn remove_volume(&self, name: &str) -> Result<(), DockerError>;
    async fn prune_volumes(&self) -> Result<Vec<String>, DockerError>;
    async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError>;
    async fn remove_network(&self, id: &str) -> Result<(), DockerError>;
    /// Runs `cmd` in the container with a TTY and stdin attached; `env` is `KEY=value` pairs.
    /// Only the exec bridge uses it (covered by an integration test), so clients that cannot
    /// (the fakes) keep this default.
    async fn exec_interactive(
        &self,
        _id: &str,
        _cmd: Vec<String>,
        _env: Vec<String>,
    ) -> Result<ExecSession, DockerError> {
        Err(DockerError::Other("exec is not supported".into()))
    }
}

/// Creates a client for the local daemon (`DOCKER_HOST` if `unix://`, else the
/// default socket). Does not fail when the daemon is down; `ping` reports that.
pub fn connect() -> Arc<dyn DockerClient> {
    Arc::new(BollardClient::new())
}
