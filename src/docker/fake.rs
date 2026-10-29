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
    /// Makes `events()` fail at once while everything else keeps working.
    events_error: Option<DockerError>,
    next_error: Option<DockerError>,
    calls: Vec<String>,
    log_history: HashMap<String, Vec<LogLine>>,
    log_subs: HashMap<String, Vec<UnboundedSender<LogLine>>>,
    stats_subs: HashMap<String, Vec<UnboundedSender<RawStats>>>,
    /// Running containers stream one made-up sample per second (see `with_sample_data`).
    synthetic_stats: bool,
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

/// Pushed samples, plus a made-up one every second until the stream is closed. The first
/// one comes after a second, so a pushed sample is never preceded by an invented one.
fn with_synthetic_samples(rx: UnboundedReceiver<RawStats>, id: &str) -> DockerStream<RawStats> {
    use tokio::time::{Duration, Interval, MissedTickBehavior, interval_at};

    let seed: u64 = id.bytes().map(u64::from).sum();
    let state: (UnboundedReceiver<RawStats>, Option<Interval>, u64) = (rx, None, 0);
    futures::stream::unfold(state, move |(mut rx, tick, n)| async move {
        // Created here, not above: an interval needs the runtime that polls the stream.
        let mut tick = tick.unwrap_or_else(|| {
            let mut i = interval_at(
                tokio::time::Instant::now() + Duration::from_secs(1),
                Duration::from_secs(1),
            );
            i.set_missed_tick_behavior(MissedTickBehavior::Delay);
            i
        });
        let (item, n) = tokio::select! {
            biased;
            pushed = rx.recv() => (pushed?, n),
            _ = tick.tick() => (synthetic_sample(seed, n + 1), n + 1),
        };
        Some((Ok(item), (rx, Some(tick), n)))
    })
    .boxed()
}

/// The `n`th made-up reading of a container: CPU and memory wave up and down, the counters grow.
fn synthetic_sample(seed: u64, n: u64) -> RawStats {
    const CPUS: u64 = 4;
    const SYSTEM_TICK: u64 = 1_000_000_000;
    const MIB: u64 = 1024 * 1024;
    let phase = (n + seed) % 20;
    let wave = if phase < 10 { phase } else { 20 - phase };
    let cpu_percent = 5 + wave * 7;
    RawStats {
        cpu_total: n * SYSTEM_TICK + cpu_percent * SYSTEM_TICK / 100 / CPUS,
        precpu_total: n * SYSTEM_TICK,
        system: (n + 1) * SYSTEM_TICK,
        presystem: n * SYSTEM_TICK,
        online_cpus: CPUS as u32,
        mem_usage: (100 + wave * 6) * MIB,
        mem_inactive_file: 10 * MIB,
        mem_limit: 512 * MIB,
        net_rx: n * 20_480,
        net_tx: n * 8_192,
        blk_read: n * 4_096,
        blk_write: n * 12_288,
    }
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

    /// Daemon with 3 containers, 2 volumes (1 in use) and 4 networks. Its running containers
    /// stream a stats sample per second, so the fake UI has charts to show.
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
            synthetic_stats: true,
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
        Self::begin_locked(self.lock(), call)
    }

    /// Like `begin`, for callers that already hold the lock (e.g. to build the
    /// call name from the canonical id). A connection-level ping error makes
    /// every operation fail, as with a stopped daemon.
    fn begin_locked(
        mut st: MutexGuard<'_, State>,
        call: String,
    ) -> Result<MutexGuard<'_, State>, DockerError> {
        st.calls.push(call);
        if let Some(e) = st.next_error.take() {
            return Err(e);
        }
        if let Some(e) = st.ping_error.clone().filter(DockerError::is_connection) {
            return Err(e);
        }
        Ok(st)
    }

    /// For stream constructors: records the call and returns an error to emit
    /// as the only item (pending `fail_next`, or a connection-level ping error).
    fn begin_stream(st: &mut State, call: String) -> Option<DockerError> {
        st.calls.push(call);
        st.next_error
            .take()
            .or_else(|| st.ping_error.clone().filter(DockerError::is_connection))
    }

    /// A connection-level error simulates a stopped daemon: every operation
    /// fails and open streams end. `None` restores normal behavior.
    pub fn set_ping_error(&self, e: Option<DockerError>) {
        let mut st = self.lock();
        if e.as_ref().is_some_and(DockerError::is_connection) {
            st.log_subs.clear();
            st.stats_subs.clear();
            st.event_subs.clear();
        }
        st.ping_error = e;
    }

    /// While set, `events()` yields `e` as its only item (the daemon answers pings and lists,
    /// but cannot stream events). `None` restores normal behavior.
    pub fn set_events_error(&self, e: Option<DockerError>) {
        self.lock().events_error = e;
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
        let id = canonical_id(&st, id);
        st.log_history
            .entry(id.clone())
            .or_default()
            .push(line.clone());
        if let Some(subs) = st.log_subs.get_mut(&id) {
            subs.retain(|tx| tx.send(line.clone()).is_ok());
        }
    }

    /// Delivers a stats sample to the open stats streams of the container.
    pub fn push_stats(&self, id: &str, raw: RawStats) {
        let mut st = self.lock();
        let id = canonical_id(&st, id);
        if let Some(subs) = st.stats_subs.get_mut(&id) {
            subs.retain(|tx| tx.send(raw.clone()).is_ok());
        }
    }

    /// Ends the live logs and stats streams of a container.
    pub fn close_streams(&self, id: &str) {
        let mut st = self.lock();
        let id = canonical_id(&st, id);
        st.log_subs.remove(&id);
        st.stats_subs.remove(&id);
    }

    /// start / stop / restart with the events the real daemon emits.
    fn lifecycle(&self, verb: &str, id: &str) -> Result<(), DockerError> {
        let mut st = self.begin(format!("{verb}:{id}"))?;
        let i = container_index(&st, id)?;
        let cid = st.containers[i].id.clone();
        let was_active = is_active(&st.containers[i].state);
        let (actions, new): (&[&str], ContainerState) = match verb {
            "start" if st.containers[i].state == ContainerState::Running => return Ok(()),
            "start" => (&["start"], ContainerState::Running),
            "stop" if !was_active => return Ok(()),
            "stop" => (&["die", "stop"], ContainerState::Exited { code: 0 }),
            _ if was_active => (&["die", "start", "restart"], ContainerState::Running),
            _ => (&["start", "restart"], ContainerState::Running),
        };
        let c = &mut st.containers[i];
        c.status_text = match new {
            ContainerState::Running => "Up less than a second".into(),
            _ => "Exited (0) just now".into(),
        };
        c.state = new;
        for action in actions {
            if *action == "die" {
                // Followed logs/stats end when the container stops running.
                st.log_subs.remove(&cid);
                st.stats_subs.remove(&cid);
            }
            emit(&mut st, ResourceKind::Container, action, &cid);
        }
        Ok(())
    }
}

/// Sends an event to every live subscriber.
fn emit(st: &mut State, kind: ResourceKind, action: &str, id: &str) {
    let e = DockerEvent {
        kind,
        action: action.into(),
        id: id.into(),
    };
    st.event_subs.retain(|tx| tx.send(e.clone()).is_ok());
}

/// Resolves an exact id, an exact name, or a unique id prefix (like Docker).
fn container_index(st: &State, id: &str) -> Result<usize, DockerError> {
    let not_found = || DockerError::NotFound(format!("No such container: {id}"));
    if id.is_empty() {
        return Err(not_found());
    }
    if let Some(i) = st
        .containers
        .iter()
        .position(|c| c.id == id || c.name == id)
    {
        return Ok(i);
    }
    let mut hits = st
        .containers
        .iter()
        .enumerate()
        .filter(|(_, c)| c.id.starts_with(id));
    match (hits.next(), hits.next()) {
        (Some((i, _)), None) => Ok(i),
        _ => Err(not_found()),
    }
}

/// Canonical id for a name/prefix; unknown ids are kept as given.
fn canonical_id(st: &State, id: &str) -> String {
    container_index(st, id).map_or_else(|_| id.to_string(), |i| st.containers[i].id.clone())
}

fn find_container<'a>(st: &'a mut State, id: &str) -> Result<&'a mut ContainerInfo, DockerError> {
    let i = container_index(st, id)?;
    Ok(&mut st.containers[i])
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
        self.lifecycle("start", id)
    }

    async fn stop(&self, id: &str) -> Result<(), DockerError> {
        self.lifecycle("stop", id)
    }

    async fn restart(&self, id: &str) -> Result<(), DockerError> {
        self.lifecycle("restart", id)
    }

    async fn remove(&self, id: &str, force: bool) -> Result<(), DockerError> {
        let st = self.lock();
        let call = format!(
            "remove:{}{}",
            canonical_id(&st, id),
            if force { ":force" } else { "" }
        );
        let mut st = Self::begin_locked(st, call)?;
        let c = find_container(&mut st, id)?;
        let (cid, name, active) = (c.id.clone(), c.name.clone(), is_active(&c.state));
        if active && !force {
            return Err(DockerError::Conflict(format!(
                "You cannot remove a running container {id}. Stop the container before attempting removal or force remove"
            )));
        }
        st.containers.retain(|c| c.id != cid);
        for v in &mut st.volumes {
            v.used_by.retain(|n| *n != name);
        }
        for n in &mut st.networks {
            n.members.retain(|m| *m != name);
        }
        st.log_subs.remove(&cid);
        st.stats_subs.remove(&cid);
        st.log_history.remove(&cid);
        emit(&mut st, ResourceKind::Container, "destroy", &cid);
        Ok(())
    }

    fn logs(&self, id: &str, tail: usize) -> DockerStream<LogLine> {
        let mut st = self.lock();
        if let Some(e) = Self::begin_stream(&mut st, format!("logs:{id}")) {
            return futures::stream::once(async move { Err(e) }).boxed();
        }
        let cid = match container_index(&st, id) {
            Ok(i) => st.containers[i].id.clone(),
            Err(err) => return futures::stream::once(async move { Err(err) }).boxed(),
        };
        let (tx, rx) = unbounded_channel();
        if let Some(history) = st.log_history.get(&cid) {
            let skip = history.len().saturating_sub(tail);
            for line in &history[skip..] {
                let _ = tx.send(line.clone());
            }
        }
        let subs = st.log_subs.entry(cid).or_default();
        subs.retain(|tx| !tx.is_closed());
        subs.push(tx);
        into_stream(rx)
    }

    fn stats(&self, id: &str) -> DockerStream<RawStats> {
        let mut st = self.lock();
        if let Some(e) = Self::begin_stream(&mut st, format!("stats:{id}")) {
            return futures::stream::once(async move { Err(e) }).boxed();
        }
        let cid = match container_index(&st, id) {
            Ok(i) => st.containers[i].id.clone(),
            Err(err) => return futures::stream::once(async move { Err(err) }).boxed(),
        };
        let (tx, rx) = unbounded_channel();
        let synthetic = st.synthetic_stats
            && st
                .containers
                .iter()
                .any(|c| c.id == cid && c.state.is_running());
        let subs = st.stats_subs.entry(cid.clone()).or_default();
        subs.retain(|tx| !tx.is_closed());
        subs.push(tx);
        if synthetic {
            with_synthetic_samples(rx, &cid)
        } else {
            into_stream(rx)
        }
    }

    fn events(&self) -> DockerStream<DockerEvent> {
        let mut st = self.lock();
        let events_error = st.events_error.clone();
        if let Some(e) = Self::begin_stream(&mut st, "events".into()).or(events_error) {
            return futures::stream::once(async move { Err(e) }).boxed();
        }
        st.event_subs.retain(|tx| !tx.is_closed());
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
        emit(&mut st, ResourceKind::Volume, "destroy", name);
        Ok(())
    }

    async fn prune_volumes(&self) -> Result<Vec<String>, DockerError> {
        let mut st = self.begin("prune_volumes".into())?;
        let removed: Vec<String> = st
            .volumes
            .iter()
            .filter(|v| v.used_by.is_empty())
            .map(|v| v.name.clone())
            .collect();
        st.volumes.retain(|v| !v.used_by.is_empty());
        for name in &removed {
            emit(&mut st, ResourceKind::Volume, "destroy", name);
        }
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
        emit(&mut st, ResourceKind::Network, "destroy", id);
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

    #[tokio::test]
    async fn fail_next_applies_to_streams() {
        let c = FakeClient::with_sample_data();
        c.fail_next(DockerError::Other("boom".into()));
        let mut s = c.logs("web-id", 1);
        assert_eq!(s.next().await, Some(Err(DockerError::Other("boom".into()))));
        assert!(s.next().await.is_none());
        assert!(c.start("db-id").await.is_ok());
        c.set_ping_error(Some(DockerError::NotRunning));
        let mut e = c.events();
        assert_eq!(e.next().await, Some(Err(DockerError::NotRunning)));
        let mut st = c.stats("web-id");
        assert_eq!(st.next().await, Some(Err(DockerError::NotRunning)));
    }

    #[tokio::test]
    async fn stop_by_name_works() {
        let c = FakeClient::with_sample_data();
        c.stop("web").await.unwrap();
        c.stop("db-").await.unwrap();
        let all = c.list_containers(true).await.unwrap();
        assert!(all.iter().all(|x| !x.state.is_running()));
        // "w" is an ambiguous prefix (web-id, worker-id).
        assert!(matches!(c.stop("w").await, Err(DockerError::NotFound(_))));
        c.stop("wor").await.unwrap();
        assert!(matches!(c.stop("zzz").await, Err(DockerError::NotFound(_))));
    }

    #[tokio::test]
    async fn start_running_is_idempotent() {
        let c = FakeClient::with_sample_data();
        let before = c.list_containers(true).await.unwrap();
        c.start("web-id").await.unwrap();
        assert_eq!(c.list_containers(true).await.unwrap(), before);
    }

    #[tokio::test]
    async fn remove_by_name_removes_and_closes_streams() {
        let c = FakeClient::with_sample_data();
        let mut s = c.logs("web", 0);
        c.remove("web", true).await.unwrap();
        assert!(s.next().await.is_none());
        let all = c.list_containers(true).await.unwrap();
        assert!(all.iter().all(|x| x.id != "web-id"));
    }

    #[tokio::test]
    async fn logs_by_name_receive_push() {
        let c = FakeClient::with_sample_data();
        let mut s = c.logs("web", 0);
        c.push_log("web-id", line("x"));
        assert_eq!(s.next().await.unwrap().unwrap().text, "x");
        c.push_log("web", line("y"));
        assert_eq!(s.next().await.unwrap().unwrap().text, "y");
        c.close_streams("web");
        assert!(s.next().await.is_none());
    }

    async fn drain(
        s: &mut DockerStream<DockerEvent>,
        n: usize,
    ) -> Vec<(ResourceKind, String, String)> {
        let mut out = vec![];
        for _ in 0..n {
            let ev = tokio::time::timeout(std::time::Duration::from_millis(200), s.next())
                .await
                .expect("event expected")
                .unwrap()
                .unwrap();
            out.push((ev.kind, ev.action, ev.id));
        }
        out
    }

    async fn assert_quiet(s: &mut DockerStream<DockerEvent>) {
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), s.next())
                .await
                .is_err(),
            "no event expected"
        );
    }

    fn ev(kind: ResourceKind, action: &str, id: &str) -> (ResourceKind, String, String) {
        (kind, action.into(), id.into())
    }

    #[tokio::test]
    async fn remove_records_canonical_id_and_force() {
        let c = FakeClient::with_sample_data();
        c.remove("worker", false).await.unwrap();
        c.remove("web", true).await.unwrap();
        let calls = c.calls();
        assert!(calls.contains(&"remove:worker-id".to_string()), "{calls:?}");
        assert!(
            calls.contains(&"remove:web-id:force".to_string()),
            "{calls:?}"
        );
    }

    #[tokio::test]
    async fn start_emits_start_event_only_when_changed() {
        let c = FakeClient::with_sample_data();
        let mut e = c.events();
        c.start("web-id").await.unwrap();
        assert_quiet(&mut e).await;
        c.start("worker").await.unwrap();
        assert_eq!(
            drain(&mut e, 1).await,
            vec![ev(ResourceKind::Container, "start", "worker-id")]
        );
    }

    #[tokio::test]
    async fn stop_emits_die_then_stop() {
        let c = FakeClient::with_sample_data();
        let mut e = c.events();
        c.stop("web").await.unwrap();
        assert_eq!(
            drain(&mut e, 2).await,
            vec![
                ev(ResourceKind::Container, "die", "web-id"),
                ev(ResourceKind::Container, "stop", "web-id")
            ]
        );
        c.stop("web").await.unwrap();
        assert_quiet(&mut e).await;
    }

    #[tokio::test]
    async fn restart_emits_die_start_restart() {
        let c = FakeClient::with_sample_data();
        let mut e = c.events();
        c.restart("web-id").await.unwrap();
        let a = |x: &str| ev(ResourceKind::Container, x, "web-id");
        assert_eq!(
            drain(&mut e, 3).await,
            vec![a("die"), a("start"), a("restart")]
        );
    }

    #[tokio::test]
    async fn removals_and_prune_emit_destroy() {
        let c = FakeClient::with_sample_data();
        let mut e = c.events();
        c.remove("worker", false).await.unwrap();
        c.remove_volume("cache").await.unwrap();
        let net = c.list_networks().await.unwrap();
        assert!(net.iter().any(|n| n.name == "app_net"));
        c.stop("web").await.unwrap();
        c.stop("db").await.unwrap();
        c.remove("web", false).await.unwrap();
        c.remove("db", false).await.unwrap();
        c.remove_network("app_net-id").await.unwrap();
        let all = drain(&mut e, 1 + 1 + 4 + 2 + 1).await;
        assert_eq!(all[0], ev(ResourceKind::Container, "destroy", "worker-id"));
        assert_eq!(all[1], ev(ResourceKind::Volume, "destroy", "cache"));
        assert_eq!(all[8], ev(ResourceKind::Network, "destroy", "app_net-id"));
        let c = FakeClient::with_sample_data();
        let mut e = c.events();
        c.prune_volumes().await.unwrap();
        assert_eq!(
            drain(&mut e, 1).await,
            vec![ev(ResourceKind::Volume, "destroy", "cache")]
        );
    }

    #[tokio::test]
    async fn connection_ping_error_simulates_stopped_daemon() {
        let c = FakeClient::with_sample_data();
        let mut ev_s = c.events();
        let mut logs = c.logs("web-id", 0);
        let mut stats = c.stats("web-id");
        c.set_ping_error(Some(DockerError::NotRunning));
        assert_eq!(c.list_containers(true).await, Err(DockerError::NotRunning));
        assert_eq!(c.start("web-id").await, Err(DockerError::NotRunning));
        assert_eq!(c.list_volumes().await, Err(DockerError::NotRunning));
        assert_eq!(c.list_networks().await, Err(DockerError::NotRunning));
        assert!(ev_s.next().await.is_none());
        assert!(logs.next().await.is_none());
        assert!(stats.next().await.is_none());
        c.set_ping_error(None);
        assert!(c.list_containers(true).await.is_ok());
        assert!(c.start("worker-id").await.is_ok());
    }

    #[tokio::test]
    async fn events_error_makes_the_stream_fail_at_once() {
        let c = FakeClient::with_sample_data();
        c.set_events_error(Some(DockerError::Other("boom".into())));
        let mut s = c.events();
        assert_eq!(
            s.next().await.unwrap(),
            Err(DockerError::Other("boom".into()))
        );
        assert!(c.ping().await.is_ok());
        assert!(c.list_containers(true).await.is_ok());
        c.set_events_error(None);
        let calls_before = c.calls().len();
        let _open = c.events();
        assert_eq!(c.calls().len(), calls_before + 1);
    }

    #[tokio::test]
    async fn non_connection_ping_error_only_affects_ping() {
        let c = FakeClient::with_sample_data();
        c.set_ping_error(Some(DockerError::Other("x".into())));
        assert!(c.list_containers(true).await.is_ok());
        assert!(c.ping().await.is_err());
    }

    #[tokio::test]
    async fn stop_ends_logs_and_stats_streams() {
        let c = FakeClient::with_sample_data();
        let mut logs = c.logs("web-id", 0);
        let mut stats = c.stats("web-id");
        let mut other = c.logs("db-id", 0);
        c.stop("web-id").await.unwrap();
        assert!(logs.next().await.is_none());
        assert!(stats.next().await.is_none());
        c.push_log("db-id", line("still"));
        assert_eq!(other.next().await.unwrap().unwrap().text, "still");
    }

    /// One running container and no sample data: nothing happens unless the test pushes it.
    fn quiet_web() -> FakeClient {
        let c = FakeClient::new();
        c.lock().containers.push(container(
            "web-id",
            "web",
            "nginx",
            ContainerState::Running,
            "Up",
            vec![],
        ));
        c
    }

    fn raw_stats(mem: u64) -> RawStats {
        RawStats {
            cpu_total: 0,
            precpu_total: 0,
            system: 0,
            presystem: 0,
            online_cpus: 1,
            mem_usage: mem,
            mem_inactive_file: 0,
            mem_limit: 1000,
            net_rx: 0,
            net_tx: 0,
            blk_read: 0,
            blk_write: 0,
        }
    }

    #[tokio::test]
    async fn push_stats_reaches_open_streams_by_id_or_name() {
        let c = quiet_web();
        let mut s = c.stats("web-id");
        c.push_stats("web-id", raw_stats(1));
        c.push_stats("web", raw_stats(2));
        assert_eq!(s.next().await.unwrap().unwrap().mem_usage, 1);
        assert_eq!(s.next().await.unwrap().unwrap().mem_usage, 2);
        c.close_streams("web");
        assert!(s.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn sample_data_streams_one_stats_sample_per_second_for_running_containers() {
        let c = FakeClient::with_sample_data();
        let mut s = c.stats("web-id");
        let start = tokio::time::Instant::now();
        let first = s.next().await.unwrap().unwrap();
        let second = s.next().await.unwrap().unwrap();
        assert_eq!(start.elapsed(), std::time::Duration::from_secs(2));
        assert!(second.net_rx > first.net_rx, "the counters grow");
        let sample = crate::stats::sample_from_raw(&second);
        assert!(sample.cpu_percent > 0.0 && sample.mem_used > 0);
        // A pushed sample still goes first, and closing ends the stream.
        c.push_stats("web-id", raw_stats(7));
        assert_eq!(s.next().await.unwrap().unwrap().mem_usage, 7);
        c.close_streams("web-id");
        assert!(s.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_without_sample_data_makes_no_samples_by_itself() {
        let c = quiet_web();
        let mut s = c.stats("web-id");
        let waited = tokio::time::timeout(std::time::Duration::from_secs(5), s.next()).await;
        assert!(waited.is_err(), "nothing arrives unless pushed");
    }
}
