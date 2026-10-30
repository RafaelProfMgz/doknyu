//! `DockerClient` implementation backed by the `bollard` crate.

use std::collections::HashMap;

use ::bollard::Docker;
use ::bollard::exec::{ResizeExecOptions, StartExecOptions, StartExecResults};
use ::bollard::models::ExecConfig;
use ::bollard::query_parameters::{
    EventsOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder,
    RemoveContainerOptionsBuilder, StatsOptionsBuilder, StopContainerOptionsBuilder,
};
use async_trait::async_trait;
use futures::{StreamExt, stream};

use super::convert::{self, api_error_with_socket};
use super::{DockerClient, DockerStream, ExecControl, ExecSession};
use crate::error::DockerError;
use crate::model::*;

pub struct BollardClient {
    /// Explicit socket path; `None` follows `DOCKER_HOST` / the default socket.
    socket: Option<String>,
}

impl BollardClient {
    pub fn new() -> Self {
        Self { socket: None }
    }

    /// Client bound to a specific unix socket, ignoring `DOCKER_HOST`.
    pub fn with_socket(path: impl Into<String>) -> Self {
        Self {
            socket: Some(path.into()),
        }
    }

    /// Socket path used for classifying connection errors.
    fn socket_path(&self) -> String {
        self.socket
            .clone()
            .unwrap_or_else(convert::docker_socket_path)
    }

    /// Error mapper bound to this client's socket.
    fn err(&self) -> impl Fn(::bollard::errors::Error) -> DockerError + Clone + Send + 'static {
        let sock = self.socket_path();
        move |e| api_error_with_socket(e, &sock)
    }

    /// Builds a handle to the daemon. Done per call so that a daemon (and its
    /// socket) appearing after startup is picked up, and a missing socket
    /// surfaces as a `DockerError` from the first operation, not from `connect`.
    fn docker(&self) -> Result<Docker, DockerError> {
        // `connect_with_unix_defaults` would not try the rootless socket.
        Docker::connect_with_unix(&self.socket_path(), 120, ::bollard::API_DEFAULT_VERSION)
            .map_err(self.err())
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
fn ignore_not_modified(
    r: Result<(), ::bollard::errors::Error>,
    err: impl Fn(::bollard::errors::Error) -> DockerError,
) -> Result<(), DockerError> {
    match r {
        Err(::bollard::errors::Error::DockerResponseServerError {
            status_code: 304, ..
        }) => Ok(()),
        other => other.map_err(err),
    }
}

/// API >= 1.42 prunes only anonymous volumes unless `all=true` is given.
fn prune_volumes_options() -> ::bollard::query_parameters::PruneVolumesOptions {
    let filters = HashMap::from([("all".to_string(), vec!["true".to_string()])]);
    ::bollard::query_parameters::PruneVolumesOptionsBuilder::new()
        .filters(&filters)
        .build()
}

impl Default for BollardClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DockerClient for BollardClient {
    async fn ping(&self) -> Result<(), DockerError> {
        self.docker()?.ping().await.map_err(self.err())?;
        Ok(())
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>, DockerError> {
        let opts = ListContainersOptionsBuilder::new().all(all).build();
        let list = self
            .docker()?
            .list_containers(Some(opts))
            .await
            .map_err(self.err())?;
        Ok(list.iter().map(convert::container_from_summary).collect())
    }

    async fn start(&self, id: &str) -> Result<(), DockerError> {
        ignore_not_modified(self.docker()?.start_container(id, None).await, self.err())
    }

    async fn stop(&self, id: &str) -> Result<(), DockerError> {
        let opts = StopContainerOptionsBuilder::new().t(10).build();
        ignore_not_modified(
            self.docker()?.stop_container(id, Some(opts)).await,
            self.err(),
        )
    }

    async fn restart(&self, id: &str) -> Result<(), DockerError> {
        ignore_not_modified(self.docker()?.restart_container(id, None).await, self.err())
    }

    async fn remove(&self, id: &str, force: bool) -> Result<(), DockerError> {
        let opts = RemoveContainerOptionsBuilder::new().force(force).build();
        self.docker()?
            .remove_container(id, Some(opts))
            .await
            .map_err(self.err())
    }

    fn logs(&self, id: &str, tail: usize) -> DockerStream<LogLine> {
        let id = id.to_string();
        let err = self.err();
        self.stream_of(move |d| {
            let opts = LogsOptionsBuilder::new()
                .follow(true)
                .stdout(true)
                .stderr(true)
                .timestamps(true)
                .tail(&tail.to_string())
                .build();
            d.logs(&id, Some(opts))
                .map(move |r| r.map(|o| convert::log_line_from(o, true)).map_err(&err))
        })
    }

    fn stats(&self, id: &str) -> DockerStream<RawStats> {
        let id = id.to_string();
        let err = self.err();
        self.stream_of(move |d| {
            let opts = StatsOptionsBuilder::new().stream(true).build();
            d.stats(&id, Some(opts))
                .map(move |r| r.map(|s| convert::raw_stats_from(&s)).map_err(&err))
        })
    }

    fn events(&self) -> DockerStream<DockerEvent> {
        let err = self.err();
        self.stream_of(move |d| {
            let filters: HashMap<String, Vec<String>> = HashMap::from([(
                "type".to_string(),
                vec!["container".into(), "volume".into(), "network".into()],
            )]);
            let opts = EventsOptionsBuilder::new().filters(&filters).build();
            d.events(Some(opts)).filter_map(move |r| {
                let item = match r {
                    Ok(m) => convert::event_from(&m).map(Ok),
                    Err(e) => Some(Err(err(e))),
                };
                async move { item }
            })
        })
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError> {
        let docker = self.docker()?;
        let volumes = docker
            .list_volumes(None::<::bollard::query_parameters::ListVolumesOptions>)
            .await
            .map_err(self.err())?
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
            .map_err(self.err())
    }

    async fn prune_volumes(&self) -> Result<Vec<String>, DockerError> {
        let resp = self
            .docker()?
            .prune_volumes(Some(prune_volumes_options()))
            .await
            .map_err(self.err())?;
        Ok(resp.volumes_deleted.unwrap_or_default())
    }

    /// Fills `containers` by crossing `list_containers(all=true)` with each
    /// container's `network_settings.networks`: two API calls in total instead
    /// of one `inspect_network` per network. Like `docker network inspect`,
    /// only running/paused/restarting containers count as connected.
    async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError> {
        let docker = self.docker()?;
        let networks = docker.list_networks(None).await.map_err(self.err())?;
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
        self.docker()?.remove_network(id).await.map_err(self.err())
    }

    async fn exec_interactive(
        &self,
        id: &str,
        cmd: Vec<String>,
        env: Vec<String>,
    ) -> Result<ExecSession, DockerError> {
        let docker = self.docker()?;
        let err = self.err();
        let config = ExecConfig {
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(true),
            env: Some(env),
            cmd: Some(cmd),
            ..Default::default()
        };
        let created = docker.create_exec(id, config).await.map_err(err.clone())?;
        let options = StartExecOptions {
            detach: false,
            tty: true,
            output_capacity: Some(16 * 1024),
        };
        match docker
            .start_exec(&created.id, Some(options))
            .await
            .map_err(err.clone())?
        {
            StartExecResults::Attached { output, input } => Ok(ExecSession {
                output: output
                    .map(move |r| {
                        r.map(|chunk| chunk.into_bytes().to_vec())
                            .map_err(err.clone())
                    })
                    .boxed(),
                input,
                control: Box::new(BollardExec {
                    docker,
                    exec_id: created.id,
                    err: Box::new(self.err()),
                }),
            }),
            StartExecResults::Detached => {
                Err(DockerError::Other("the exec started detached".into()))
            }
        }
    }
}

/// Resize and exit code of one exec.
struct BollardExec {
    docker: Docker,
    exec_id: String,
    err: Box<dyn Fn(::bollard::errors::Error) -> DockerError + Send + Sync>,
}

#[async_trait]
impl ExecControl for BollardExec {
    async fn resize(&self, cols: u16, rows: u16) -> Result<(), DockerError> {
        let options = ResizeExecOptions {
            width: cols,
            height: rows,
        };
        self.docker
            .resize_exec(&self.exec_id, options)
            .await
            .map_err(&self.err)
    }

    async fn exit_code(&self) -> Result<Option<i64>, DockerError> {
        // The output can end a moment before the daemon records the exit.
        for _ in 0..20 {
            let info = self
                .docker
                .inspect_exec(&self.exec_id)
                .await
                .map_err(&self.err)?;
            if info.running != Some(true) {
                return Ok(info.exit_code);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Ok(None)
    }
}

impl BollardClient {
    async fn list_containers_raw(
        &self,
        docker: &Docker,
    ) -> Result<Vec<::bollard::models::ContainerSummary>, DockerError> {
        let opts = ListContainersOptionsBuilder::new().all(true).build();
        docker.list_containers(Some(opts)).await.map_err(self.err())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_volumes_options_include_all_filter() {
        let opts = prune_volumes_options();
        let filters = opts.filters.expect("filters set");
        assert_eq!(filters.get("all"), Some(&vec!["true".to_string()]));
    }
}
