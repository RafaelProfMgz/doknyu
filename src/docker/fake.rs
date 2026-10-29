//! In-memory `DockerClient` used by tests and by the UI when running without Docker.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::{DockerClient, DockerStream};
use crate::error::DockerError;
use crate::model::*;

const PROTECTED_NETWORKS: [&str; 3] = ["bridge", "host", "none"];

struct NetworkEntry {
    info: NetworkInfo,
    /// Names of every container attached to the network (running or not).
    members: Vec<String>,
}

#[derive(Default)]
struct State {
    containers: Vec<ContainerInfo>,
    volumes: Vec<VolumeInfo>,
    networks: Vec<NetworkEntry>,
    ping_error: Option<DockerError>,
    next_error: Option<DockerError>,
    calls: Vec<String>,
    log_history: HashMap<String, Vec<LogLine>>,
    log_subs: HashMap<String, Vec<UnboundedSender<LogLine>>>,
    stats_subs: HashMap<String, Vec<UnboundedSender<RawStats>>>,
    event_subs: Vec<UnboundedSender<DockerEvent>>,
}

/// Fake Docker daemon with controllable streams and failures.
#[derive(Default)]
pub struct FakeClient {
    state: Mutex<State>,
}

/// Running, paused or restarting (what Docker lists without `all`).
fn is_active(s: &ContainerState) -> bool {
    matches!(
        s,
        ContainerState::Running | ContainerState::Paused | ContainerState::Restarting
    )
}

fn into_stream<T: Send + 'static>(rx: UnboundedReceiver<T>) -> DockerStream<T> {
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (Ok(item), rx))
    })
    .boxed()
}

fn container(
    id: &str,
    name: &str,
    image: &str,
    state: ContainerState,
    status: &str,
    ports: Vec<PortMapping>,
) -> ContainerInfo {
    ContainerInfo {
        id: id.into(),
        name: name.into(),
        image: image.into(),
        state,
        status_text: status.into(),
        ports,
    }
}

fn network(name: &str, driver: &str, subnet: Option<&str>, members: &[&str]) -> NetworkEntry {
    NetworkEntry {
        info: NetworkInfo {
            id: format!("{name}-id"),
            name: name.into(),
            driver: driver.into(),
            subnet: subnet.map(String::from),
            containers: Vec::new(),
        },
        members: members.iter().map(|m| m.to_string()).collect(),
    }
}

impl FakeClient {
    /// Empty daemon.
    pub fn new() -> Self {
        Self::default()
    }

    /// Daemon with 3 containers, 2 volumes (1 in use) and 4 networks.
    pub fn with_sample_data() -> Self {
        let state = State {
            containers: vec![
                container(
                    "web-id",
                    "web",
                    "nginx:latest",
                    ContainerState::Running,
                    "Up 2 hours",
                    vec![PortMapping {
                        private: 80,
                        public: Some(8080),
                        protocol: "tcp".into(),
                    }],
                ),
                container(
                    "db-id",
                    "db",
                    "postgres:16",
                    ContainerState::Running,
                    "Up 2 hours",
                    vec![],
                ),
                container(
                    "worker-id",
                    "worker",
                    "app/worker:1.0",
                    ContainerState::Exited { code: 1 },
                    "Exited (1) 3 hours ago",
                    vec![],
                ),
            ],
            volumes: vec![
                VolumeInfo {
                    name: "db_data".into(),
                    driver: "local".into(),
                    mountpoint: "/var/lib/docker/volumes/db_data/_data".into(),
                    used_by: vec!["db".into()],
                },
                VolumeInfo {
                    name: "cache".into(),
                    driver: "local".into(),
                    mountpoint: "/var/lib/docker/volumes/cache/_data".into(),
                    used_by: vec![],
                },
            ],
            networks: vec![
                network("bridge", "bridge", Some("172.17.0.0/16"), &[]),
                network("host", "host", None, &[]),
                network("none", "null", None, &[]),
                network(
                    "app_net",
                    "bridge",
                    Some("172.20.0.0/16"),
                    &["web", "db", "worker"],
                ),
            ],
            ..State::default()
        };
        Self {
            state: Mutex::new(state),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records the call and returns the pending injected error, if any.
    fn begin(&self, call: String) -> Result<MutexGuard<'_, State>, DockerError> {
        let mut st = self.lock();
        st.calls.push(call);
        match st.next_error.take() {
            Some(e) => Err(e),
            None => Ok(st),
        }
    }

    pub fn set_ping_error(&self, e: Option<DockerError>) {
        self.lock().ping_error = e;
    }

    /// The next fallible call returns `e`, then behavior returns to normal.
    pub fn fail_next(&self, e: DockerError) {
        self.lock().next_error = Some(e);
    }

    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    /// Delivers `e` to current event subscribers (dropped if there are none).
    pub fn push_event(&self, e: DockerEvent) {
        self.lock()
            .event_subs
            .retain(|tx| tx.send(e.clone()).is_ok());
    }

    /// Appends to the container's log history and to live log streams.
    pub fn push_log(&self, id: &str, line: LogLine) {
        let mut st = self.lock();
        st.log_history
            .entry(id.to_string())
            .or_default()
            .push(line.clone());
        if let Some(subs) = st.log_subs.get_mut(id) {
            subs.retain(|tx| tx.send(line.clone()).is_ok());
        }
    }

    /// Ends the live logs and stats streams of a container.
    pub fn close_streams(&self, id: &str) {
        let mut st = self.lock();
        st.log_subs.remove(id);
        st.stats_subs.remove(id);
    }

    fn set_state(&self, call: String, id: &str, new: ContainerState) -> Result<(), DockerError> {
        let mut st = self.begin(call)?;
        let c = find_container(&mut st, id)?;
        c.status_text = match new {
            ContainerState::Running => "Up less than a second".into(),
            _ => "Exited (0) just now".into(),
        };
        c.state = new;
        Ok(())
    }
}

fn find_container<'a>(st: &'a mut State, id: &str) -> Result<&'a mut ContainerInfo, DockerError> {
    st.containers
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| DockerError::NotFound(format!("No such container: {id}")))
}

#[async_trait::async_trait]
impl DockerClient for FakeClient {
    async fn ping(&self) -> Result<(), DockerError> {
        let st = self.begin("ping".into())?;
        match &st.ping_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>, DockerError> {
        let st = self.begin(format!("list_containers:{all}"))?;
        Ok(st
            .containers
            .iter()
            .filter(|c| all || is_active(&c.state))
            .cloned()
            .collect())
    }

    async fn start(&self, id: &str) -> Result<(), DockerError> {
        self.set_state(format!("start:{id}"), id, ContainerState::Running)
    }

    async fn stop(&self, id: &str) -> Result<(), DockerError> {
        self.set_state(format!("stop:{id}"), id, ContainerState::Exited { code: 0 })
    }

    async fn restart(&self, id: &str) -> Result<(), DockerError> {
        self.set_state(format!("restart:{id}"), id, ContainerState::Running)
    }

    async fn remove(&self, id: &str, force: bool) -> Result<(), DockerError> {
        let mut st = self.begin(format!("remove:{id}"))?;
        let c = find_container(&mut st, id)?;
        let (name, active) = (c.name.clone(), is_active(&c.state));
        if active && !force {
            return Err(DockerError::Conflict(format!(
                "You cannot remove a running container {id}. Stop the container before attempting removal or force remove"
            )));
        }
        st.containers.retain(|c| c.id != id);
        for v in &mut st.volumes {
            v.used_by.retain(|n| *n != name);
        }
        for n in &mut st.networks {
            n.members.retain(|m| *m != name);
        }
        st.log_subs.remove(id);
        st.stats_subs.remove(id);
        st.log_history.remove(id);
        Ok(())
    }

    fn logs(&self, id: &str, tail: usize) -> DockerStream<LogLine> {
        let mut st = self.lock();
        st.calls.push(format!("logs:{id}"));
        if !st.containers.iter().any(|c| c.id == id) {
            let err = DockerError::NotFound(format!("No such container: {id}"));
            return futures::stream::once(async move { Err(err) }).boxed();
        }
        let (tx, rx) = unbounded_channel();
        if let Some(history) = st.log_history.get(id) {
            let skip = history.len().saturating_sub(tail);
            for line in &history[skip..] {
                let _ = tx.send(line.clone());
            }
        }
        st.log_subs.entry(id.to_string()).or_default().push(tx);
        into_stream(rx)
    }

    fn stats(&self, id: &str) -> DockerStream<RawStats> {
        let mut st = self.lock();
        st.calls.push(format!("stats:{id}"));
        if !st.containers.iter().any(|c| c.id == id) {
            let err = DockerError::NotFound(format!("No such container: {id}"));
            return futures::stream::once(async move { Err(err) }).boxed();
        }
        let (tx, rx) = unbounded_channel();
        st.stats_subs.entry(id.to_string()).or_default().push(tx);
        into_stream(rx)
    }

    fn events(&self) -> DockerStream<DockerEvent> {
        let mut st = self.lock();
        st.calls.push("events".into());
        let (tx, rx) = unbounded_channel();
        st.event_subs.push(tx);
        into_stream(rx)
    }

    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError> {
        Ok(self.begin("list_volumes".into())?.volumes.clone())
    }

    async fn remove_volume(&self, name: &str) -> Result<(), DockerError> {
        let mut st = self.begin(format!("remove_volume:{name}"))?;
        let v = st
            .volumes
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| DockerError::NotFound(format!("No such volume: {name}")))?;
        if !v.used_by.is_empty() {
            return Err(DockerError::Conflict(format!(
                "remove {name}: volume is in use"
            )));
        }
        st.volumes.retain(|v| v.name != name);
        Ok(())
    }

    async fn prune_volumes(&self) -> Result<Vec<String>, DockerError> {
        let mut st = self.begin("prune_volumes".into())?;
        let removed = st
            .volumes
            .iter()
            .filter(|v| v.used_by.is_empty())
            .map(|v| v.name.clone())
            .collect();
        st.volumes.retain(|v| !v.used_by.is_empty());
        Ok(removed)
    }

    async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError> {
        let st = self.begin("list_networks".into())?;
        Ok(st
            .networks
            .iter()
            .map(|n| NetworkInfo {
                containers: st
                    .containers
                    .iter()
                    .filter(|c| is_active(&c.state) && n.members.contains(&c.name))
                    .map(|c| c.name.clone())
                    .collect(),
                ..n.info.clone()
            })
            .collect())
    }

    async fn remove_network(&self, id: &str) -> Result<(), DockerError> {
        let mut st = self.begin(format!("remove_network:{id}"))?;
        let n = st
            .networks
            .iter()
            .find(|n| n.info.id == id)
            .ok_or_else(|| DockerError::NotFound(format!("No such network: {id}")))?;
        if PROTECTED_NETWORKS.contains(&n.info.name.as_str()) {
            return Err(DockerError::Api {
                status: 403,
                message: format!(
                    "{} is a pre-defined network and cannot be removed",
                    n.info.name
                ),
            });
        }
        let in_use = st
            .containers
            .iter()
            .any(|c| is_active(&c.state) && n.members.contains(&c.name));
        if in_use {
            return Err(DockerError::Conflict(format!(
                "network {} has active endpoints",
                n.info.name
            )));
        }
        st.networks.retain(|n| n.info.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn line(text: &str) -> LogLine {
        LogLine {
            stream: LogStream::Stdout,
            timestamp: None,
            text: text.into(),
        }
    }

    #[tokio::test]
    async fn stop_changes_state() {
        let c = FakeClient::with_sample_data();
        c.stop("web-id").await.unwrap();
        let all = c.list_containers(true).await.unwrap();
        let web = all.iter().find(|x| x.id == "web-id").unwrap();
        assert_eq!(web.state, ContainerState::Exited { code: 0 });
        assert!(c.calls().contains(&"stop:web-id".to_string()));
        assert_eq!(c.list_containers(false).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn remove_running_without_force_conflicts() {
        let c = FakeClient::with_sample_data();
        assert!(matches!(
            c.remove("web-id", false).await,
            Err(DockerError::Conflict(_))
        ));
        c.remove("web-id", true).await.unwrap();
        assert!(matches!(
            c.remove("web-id", true).await,
            Err(DockerError::NotFound(_))
        ));
        c.remove("worker-id", false).await.unwrap();
    }

    #[tokio::test]
    async fn remove_in_use_volume_conflicts() {
        let c = FakeClient::with_sample_data();
        let vols = c.list_volumes().await.unwrap();
        let used = vols.iter().find(|v| !v.used_by.is_empty()).unwrap();
        let free = vols.iter().find(|v| v.used_by.is_empty()).unwrap();
        assert!(matches!(
            c.remove_volume(&used.name).await,
            Err(DockerError::Conflict(_))
        ));
        c.remove_volume(&free.name).await.unwrap();
        assert!(matches!(
            c.remove_volume("nope").await,
            Err(DockerError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn fail_next_returns_error_once() {
        let c = FakeClient::with_sample_data();
        c.fail_next(DockerError::Other("boom".into()));
        assert_eq!(
            c.start("db-id").await,
            Err(DockerError::Other("boom".into()))
        );
        assert!(c.start("db-id").await.is_ok());
    }

    #[tokio::test]
    async fn ping_error_is_configurable() {
        let c = FakeClient::new();
        assert!(c.ping().await.is_ok());
        c.set_ping_error(Some(DockerError::NotRunning));
        assert_eq!(c.ping().await, Err(DockerError::NotRunning));
    }

    #[tokio::test]
    async fn logs_stream_ends_on_close() {
        let c = FakeClient::with_sample_data();
        let mut s = c.logs("web-id", 10);
        c.push_log("web-id", line("a"));
        assert_eq!(s.next().await.unwrap().unwrap().text, "a");
        c.close_streams("web-id");
        assert!(s.next().await.is_none());
    }

    #[tokio::test]
    async fn push_before_subscribe_does_not_panic() {
        let c = FakeClient::with_sample_data();
        c.push_event(DockerEvent {
            kind: ResourceKind::Container,
            action: "die".into(),
            id: "web-id".into(),
        });
        c.push_log("web-id", line("early"));
        c.close_streams("web-id");
        let mut s = c.logs("web-id", 10);
        assert_eq!(s.next().await.unwrap().unwrap().text, "early");
    }

    #[tokio::test]
    async fn events_are_delivered_to_subscribers() {
        let c = FakeClient::new();
        let mut s = c.events();
        let ev = DockerEvent {
            kind: ResourceKind::Volume,
            action: "create".into(),
            id: "v".into(),
        };
        c.push_event(ev.clone());
        assert_eq!(s.next().await.unwrap().unwrap(), ev);
    }

    #[tokio::test]
    async fn removing_container_ends_its_streams() {
        let c = FakeClient::with_sample_data();
        let mut s = c.logs("worker-id", 0);
        c.remove("worker-id", false).await.unwrap();
        assert!(s.next().await.is_none());
        let mut gone = c.stats("worker-id");
        assert!(matches!(
            gone.next().await,
            Some(Err(DockerError::NotFound(_)))
        ));
    }

    #[tokio::test]
    async fn networks_list_only_active_containers() {
        let c = FakeClient::with_sample_data();
        let nets = c.list_networks().await.unwrap();
        let app = nets.iter().find(|n| n.name == "app_net").unwrap();
        assert_eq!(app.containers, vec!["web".to_string(), "db".to_string()]);
        assert!(matches!(
            c.remove_network(&app.id).await,
            Err(DockerError::Conflict(_))
        ));
        assert!(c.remove_network("bridge-id").await.is_err());
    }

    #[tokio::test]
    async fn prune_removes_unused_volumes() {
        let c = FakeClient::with_sample_data();
        let removed = c.prune_volumes().await.unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(c.list_volumes().await.unwrap().len(), 1);
    }
}
