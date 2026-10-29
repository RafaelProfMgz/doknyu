//! `DockerClient` implementation backed by the `bollard` crate.

use std::collections::HashMap;

use ::bollard::Docker;
use ::bollard::query_parameters::{
    EventsOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder,
    RemoveContainerOptionsBuilder, StatsOptionsBuilder, StopContainerOptionsBuilder,
};
use async_trait::async_trait;
use futures::{StreamExt, stream};

use super::convert::{self, api_error_from};
use super::{DockerClient, DockerStream};
use crate::error::DockerError;
use crate::model::*;

pub struct BollardClient;

impl BollardClient {
    pub fn new() -> Self {
        Self
    }

    /// Builds a handle to the daemon. Done per call so that a daemon (and its
    /// socket) appearing after startup is picked up, and a missing socket
    /// surfaces as a `DockerError` from the first operation, not from `connect`.
    fn docker(&self) -> Result<Docker, DockerError> {
        Docker::connect_with_unix_defaults().map_err(api_error_from)
    }

    /// Wraps a stream builder so a connection failure becomes a one-item error stream.
    fn stream_of<T, S, F>(&self, build: F) -> DockerStream<T>
    where
        T: Send + 'static,
        S: stream::Stream<Item = Result<T, DockerError>> + Send + 'static,
        F: FnOnce(Docker) -> S,
    {
        match self.docker() {
            Ok(d) => build(d).boxed(),
            Err(e) => stream::once(async move { Err(e) }).boxed(),
        }
    }
}

/// Docker answers 304 when the container is already in the requested state;
/// treat that as success so start/stop are idempotent.
fn ignore_not_modified(r: Result<(), ::bollard::errors::Error>) -> Result<(), DockerError> {
    match r {
        Err(::bollard::errors::Error::DockerResponseServerError {
            status_code: 304, ..
        }) => Ok(()),
        other => other.map_err(api_error_from),
    }
}

impl Default for BollardClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DockerClient for BollardClient {
    async fn ping(&self) -> Result<(), DockerError> {
        self.docker()?.ping().await.map_err(api_error_from)?;
        Ok(())
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>, DockerError> {
        let opts = ListContainersOptionsBuilder::new().all(all).build();
        let list = self
            .docker()?
            .list_containers(Some(opts))
            .await
            .map_err(api_error_from)?;
        Ok(list.iter().map(convert::container_from_summary).collect())
    }

    async fn start(&self, id: &str) -> Result<(), DockerError> {
        ignore_not_modified(self.docker()?.start_container(id, None).await)
    }

    async fn stop(&self, id: &str) -> Result<(), DockerError> {
        let opts = StopContainerOptionsBuilder::new().t(10).build();
        ignore_not_modified(self.docker()?.stop_container(id, Some(opts)).await)
    }

    async fn restart(&self, id: &str) -> Result<(), DockerError> {
        ignore_not_modified(self.docker()?.restart_container(id, None).await)
    }

    async fn remove(&self, id: &str, force: bool) -> Result<(), DockerError> {
        let opts = RemoveContainerOptionsBuilder::new().force(force).build();
        self.docker()?
            .remove_container(id, Some(opts))
            .await
            .map_err(api_error_from)
    }

    fn logs(&self, id: &str, tail: usize) -> DockerStream<LogLine> {
        let id = id.to_string();
        self.stream_of(move |d| {
            let opts = LogsOptionsBuilder::new()
                .follow(true)
                .stdout(true)
                .stderr(true)
                .timestamps(true)
                .tail(&tail.to_string())
                .build();
            d.logs(&id, Some(opts)).map(|r| {
                r.map(|o| convert::log_line_from(o, true))
                    .map_err(api_error_from)
            })
        })
    }

    fn stats(&self, id: &str) -> DockerStream<RawStats> {
        let id = id.to_string();
        self.stream_of(move |d| {
            let opts = StatsOptionsBuilder::new().stream(true).build();
            d.stats(&id, Some(opts)).map(|r| {
                r.map(|s| convert::raw_stats_from(&s))
                    .map_err(api_error_from)
            })
        })
    }

    fn events(&self) -> DockerStream<DockerEvent> {
        self.stream_of(|d| {
            let filters: HashMap<String, Vec<String>> = HashMap::from([(
                "type".to_string(),
                vec!["container".into(), "volume".into(), "network".into()],
            )]);
            let opts = EventsOptionsBuilder::new().filters(&filters).build();
            d.events(Some(opts)).filter_map(|r| async move {
                match r {
                    Ok(m) => convert::event_from(&m).map(Ok),
                    Err(e) => Some(Err(api_error_from(e))),
                }
            })
        })
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError> {
        let docker = self.docker()?;
        let volumes = docker
            .list_volumes(None::<::bollard::query_parameters::ListVolumesOptions>)
            .await
            .map_err(api_error_from)?
            .volumes
            .unwrap_or_default();
        let containers = self.list_containers_raw(&docker).await?;
        Ok(volumes
            .iter()
            .map(|v| {
                let used_by = containers
                    .iter()
                    .filter(|c| {
                        c.mounts.as_ref().is_some_and(|ms| {
                            ms.iter()
                                .any(|m| m.name.as_deref() == Some(v.name.as_str()))
                        })
                    })
                    .map(|c| convert::container_from_summary(c).name)
                    .collect();
                convert::volume_from(v, used_by)
            })
            .collect())
    }

    async fn remove_volume(&self, name: &str) -> Result<(), DockerError> {
        self.docker()?
            .remove_volume(
                name,
                None::<::bollard::query_parameters::RemoveVolumeOptions>,
            )
            .await
            .map_err(api_error_from)
    }

    async fn prune_volumes(&self) -> Result<Vec<String>, DockerError> {
        let resp = self
            .docker()?
            .prune_volumes(None::<::bollard::query_parameters::PruneVolumesOptions>)
            .await
            .map_err(api_error_from)?;
        Ok(resp.volumes_deleted.unwrap_or_default())
    }

    /// Fills `containers` by crossing `list_containers(all=true)` with each
    /// container's `network_settings.networks`: two API calls in total instead
    /// of one `inspect_network` per network. Like `docker network inspect`,
    /// only running/paused/restarting containers count as connected.
    async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError> {
        let docker = self.docker()?;
        let networks = docker.list_networks(None).await.map_err(api_error_from)?;
        let containers = self.list_containers_raw(&docker).await?;
        Ok(networks
            .iter()
            .map(|n| {
                let mut info = convert::network_from(n);
                info.containers = containers
                    .iter()
                    .filter(|c| {
                        matches!(
                            convert::container_from_summary(c).state,
                            ContainerState::Running
                                | ContainerState::Paused
                                | ContainerState::Restarting
                        )
                    })
                    .filter(|c| {
                        c.network_settings
                            .as_ref()
                            .and_then(|s| s.networks.as_ref())
                            .is_some_and(|nets| {
                                nets.iter().any(|(name, ep)| {
                                    name == &info.name
                                        || (!info.id.is_empty()
                                            && ep.network_id.as_deref() == Some(info.id.as_str()))
                                })
                            })
                    })
                    .map(|c| convert::container_from_summary(c).name)
                    .collect();
                info
            })
            .collect())
    }

    async fn remove_network(&self, id: &str) -> Result<(), DockerError> {
        self.docker()?
            .remove_network(id)
            .await
            .map_err(api_error_from)
    }
}

impl BollardClient {
    async fn list_containers_raw(
        &self,
        docker: &Docker,
    ) -> Result<Vec<::bollard::models::ContainerSummary>, DockerError> {
        let opts = ListContainersOptionsBuilder::new().all(true).build();
        docker
            .list_containers(Some(opts))
            .await
            .map_err(api_error_from)
    }
}
