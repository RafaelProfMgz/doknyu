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
}

/// Creates a client for the local daemon (`DOCKER_HOST` if `unix://`, else the
/// default socket). Does not fail when the daemon is down; `ping` reports that.
pub fn connect() -> Arc<dyn DockerClient> {
    Arc::new(BollardClient::new())
}
