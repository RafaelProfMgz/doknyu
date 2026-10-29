//! Pure conversions from bollard types into the domain model.

use crate::error::{DockerError, classify_connect_error};
use crate::model::*;
use bollard::container::LogOutput;
use bollard::models::{
    ContainerStatsResponse, ContainerSummary, ContainerSummaryStateEnum, EventMessage,
    EventMessageTypeEnum, Network, PortSummary, PortSummaryTypeEnum, Volume,
};

/// Container name without the leading slash; falls back to the 12-char short id.
fn container_name(s: &ContainerSummary) -> String {
    let name = s
        .names
        .as_ref()
        .and_then(|n| n.first())
        .map(|n| n.trim_start_matches('/'))
        .filter(|n| !n.is_empty());
    match name {
        Some(n) => n.to_string(),
        None => {
            s.id.as_deref()
                .unwrap_or_default()
                .chars()
                .take(12)
                .collect()
        }
    }
}

/// Parses the exit code out of a status like `Exited (137) 2 minutes ago`.
fn exit_code_from_status(status: &str) -> i64 {
    status
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .and_then(|(code, _)| code.trim().parse().ok())
        .unwrap_or(0)
}

fn state_from(s: &ContainerSummary) -> ContainerState {
    use ContainerSummaryStateEnum as S;
    match s.state {
        Some(S::RUNNING | S::STOPPING) => ContainerState::Running,
        Some(S::PAUSED) => ContainerState::Paused,
        Some(S::RESTARTING) => ContainerState::Restarting,
        Some(S::EXITED) => ContainerState::Exited {
            code: exit_code_from_status(s.status.as_deref().unwrap_or_default()),
        },
        // Shown as stopped (not as an error) while the removal is in progress.
        Some(S::REMOVING) => ContainerState::Exited { code: 0 },
        Some(S::DEAD) => ContainerState::Dead,
        Some(S::CREATED | S::EMPTY) | None => ContainerState::Created,
    }
}

fn ports_from(ports: &[PortSummary]) -> Vec<PortMapping> {
    let mut out: Vec<PortMapping> = Vec::new();
    for p in ports {
        let protocol = match p.typ {
            Some(PortSummaryTypeEnum::UDP) => "udp",
            Some(PortSummaryTypeEnum::SCTP) => "sctp",
            _ => "tcp",
        };
        let m = PortMapping {
            private: p.private_port,
            public: p.public_port,
            protocol: protocol.to_string(),
        };
        // The daemon reports one entry per bound address (IPv4 and IPv6).
        if !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

pub fn container_from_summary(s: &ContainerSummary) -> ContainerInfo {
    ContainerInfo {
        id: s.id.clone().unwrap_or_default(),
        name: container_name(s),
        image: s.image.clone().unwrap_or_default(),
        state: state_from(s),
        status_text: s.status.clone().unwrap_or_default(),
        ports: ports_from(s.ports.as_deref().unwrap_or_default()),
    }
}

pub fn raw_stats_from(s: &ContainerStatsResponse) -> RawStats {
    let cpu = s.cpu_stats.as_ref();
    let pre = s.precpu_stats.as_ref();
    let total = |c: Option<&bollard::models::ContainerCpuStats>| {
        c.and_then(|c| c.cpu_usage.as_ref())
            .and_then(|u| u.total_usage)
            .unwrap_or(0)
    };
    let sys = |c: Option<&bollard::models::ContainerCpuStats>| {
        c.and_then(|c| c.system_cpu_usage).unwrap_or(0)
    };
    let mem = s.memory_stats.as_ref();
    // cgroup v2 reports `inactive_file`, cgroup v1 reports `total_inactive_file`.
    let inactive = mem
        .and_then(|m| m.stats.as_ref())
        .and_then(|st| {
            st.get("inactive_file")
                .or_else(|| st.get("total_inactive_file"))
        })
        .copied()
        .unwrap_or(0);
    let (net_rx, net_tx) =
        s.networks
            .iter()
            .flat_map(|n| n.values())
            .fold((0u64, 0u64), |(rx, tx), n| {
                (
                    rx.saturating_add(n.rx_bytes.unwrap_or(0)),
                    tx.saturating_add(n.tx_bytes.unwrap_or(0)),
                )
            });
    let blk = |op: &str| {
        s.blkio_stats
            .as_ref()
            .and_then(|b| b.io_service_bytes_recursive.as_ref())
            .map(|v| {
                v.iter()
                    .filter(|e| e.op.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(op)))
                    .fold(0u64, |a, e| a.saturating_add(e.value.unwrap_or(0)))
            })
            .unwrap_or(0)
    };
    RawStats {
        cpu_total: total(cpu),
        precpu_total: total(pre),
        system: sys(cpu),
        presystem: sys(pre),
        online_cpus: cpu.and_then(|c| c.online_cpus).unwrap_or(0),
        mem_usage: mem.and_then(|m| m.usage).unwrap_or(0),
        mem_inactive_file: inactive,
        mem_limit: mem.and_then(|m| m.limit).unwrap_or(0),
        net_rx,
        net_tx,
        blk_read: blk("read"),
        blk_write: blk("write"),
    }
}

pub fn volume_from(v: &Volume, used_by: Vec<String>) -> VolumeInfo {
    VolumeInfo {
        name: v.name.clone(),
        driver: v.driver.clone(),
        mountpoint: v.mountpoint.clone(),
        used_by,
    }
}

/// `containers` is left empty: bollard's `Network` (list response) carries no
/// container attachments; the client fills them in separately.
pub fn network_from(n: &Network) -> NetworkInfo {
    let subnet = n
        .ipam
        .as_ref()
        .and_then(|i| i.config.as_ref())
        .and_then(|c| c.first())
        .and_then(|c| c.subnet.clone())
        .filter(|s| !s.is_empty());
    NetworkInfo {
        id: n.id.clone().unwrap_or_default(),
        name: n.name.clone().unwrap_or_default(),
        driver: n.driver.clone().unwrap_or_default(),
        subnet,
        containers: Vec::new(),
    }
}

pub fn log_line_from(o: LogOutput, timestamps: bool) -> LogLine {
    let (stream, message) = match o {
        LogOutput::StdErr { message } => (LogStream::Stderr, message),
        LogOutput::StdOut { message }
        | LogOutput::StdIn { message }
        | LogOutput::Console { message } => (LogStream::Stdout, message),
    };
    let full = String::from_utf8_lossy(&message);
    let full = full.trim_end_matches(['\n', '\r']);
    let (timestamp, text) = match full.split_once(' ') {
        Some((ts, rest)) if timestamps => (Some(ts.to_string()), rest),
        _ => (None, full),
    };
    LogLine {
        stream,
        timestamp,
        text: text.to_string(),
    }
}

pub fn event_from(m: &EventMessage) -> Option<DockerEvent> {
    let kind = match m.typ? {
        EventMessageTypeEnum::CONTAINER => ResourceKind::Container,
        EventMessageTypeEnum::VOLUME => ResourceKind::Volume,
        EventMessageTypeEnum::NETWORK => ResourceKind::Network,
        _ => return None,
    };
    Some(DockerEvent {
        kind,
        action: m.action.clone()?,
        id: m.actor.as_ref()?.id.clone()?,
    })
}

fn docker_socket_path() -> String {
    match std::env::var("DOCKER_HOST") {
        Ok(h) if h.starts_with("unix://") => h["unix://".len()..].to_string(),
        _ => "/var/run/docker.sock".to_string(),
    }
}

fn docker_cli_installed() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("docker").is_file()))
}

pub fn api_error_from(e: bollard::errors::Error) -> DockerError {
    use bollard::errors::Error;
    match e {
        Error::DockerResponseServerError {
            status_code,
            message,
        } => match status_code {
            404 => DockerError::NotFound(message),
            409 => DockerError::Conflict(message),
            status => DockerError::Api { status, message },
        },
        Error::IOError { err } => classify_connect_error(
            err.kind(),
            std::path::Path::new(&docker_socket_path()).exists(),
            docker_cli_installed(),
        ),
        other => DockerError::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DockerError;
    use bollard::models::{
        ContainerStatsResponse, ContainerSummary, EventMessage, Network, Volume,
    };

    const CONTAINERS: &str = include_str!("../../tests/fixtures/containers.json");
    const NO_NAME: &str = include_str!("../../tests/fixtures/container_no_name.json");
    const NETWORK: &str = include_str!("../../tests/fixtures/network.json");
    const VOLUME: &str = include_str!("../../tests/fixtures/volume.json");
    const STATS: &str = include_str!("../../tests/fixtures/stats.json");
    const EV_START: &str = include_str!("../../tests/fixtures/event_container_start.json");
    const EV_IMAGE: &str = include_str!("../../tests/fixtures/event_image_pull.json");

    fn containers() -> Vec<ContainerSummary> {
        serde_json::from_str(CONTAINERS).unwrap()
    }

    fn by_name(name: &str) -> ContainerInfo {
        let all = containers();
        let s = all
            .iter()
            .find(|c| c.names.as_ref().is_some_and(|n| n[0] == format!("/{name}")))
            .unwrap();
        container_from_summary(s)
    }

    #[test]
    fn container_name_strips_leading_slash() {
        let c = by_name("doknyu-fixture-web");
        assert_eq!(c.name, "doknyu-fixture-web");
        assert_eq!(c.image, "alpine");
        assert_eq!(c.state, ContainerState::Running);
        assert_eq!(c.status_text, "Up 3 seconds");
    }

    #[test]
    fn container_without_name_uses_short_id() {
        let s: Vec<ContainerSummary> = serde_json::from_str(NO_NAME).unwrap();
        let c = container_from_summary(&s[0]);
        assert_eq!(c.name, "abcdef123456");
        assert_eq!(c.state, ContainerState::Created);
    }

    #[test]
    fn container_exited_code_parsed() {
        let c = by_name("doknyu-fixture-fail");
        assert_eq!(c.state, ContainerState::Exited { code: 137 });
    }

    #[test]
    fn ports_dedup_ipv4_ipv6() {
        let c = by_name("doknyu-fixture-web");
        assert_eq!(
            c.ports,
            vec![PortMapping {
                private: 80,
                public: Some(18080),
                protocol: "tcp".into()
            }]
        );
    }

    #[test]
    fn network_subnet_from_ipam() {
        let n: Network = serde_json::from_str(NETWORK).unwrap();
        let info = network_from(&n);
        assert_eq!(info.name, "doknyu-fixture-net");
        assert_eq!(info.driver, "bridge");
        assert_eq!(info.subnet.as_deref(), Some("172.25.0.0/16"));
        let none = Network::default();
        assert_eq!(network_from(&none).subnet, None);
    }

    #[test]
    fn volume_fields_and_used_by() {
        let v: Volume = serde_json::from_str(VOLUME).unwrap();
        let info = volume_from(&v, vec!["web".into()]);
        assert_eq!(info.name, "doknyu-fixture-vol");
        assert_eq!(info.driver, "local");
        assert_eq!(info.used_by, vec!["web".to_string()]);
    }

    #[test]
    fn event_ignores_unknown_types() {
        let img: EventMessage = serde_json::from_str(EV_IMAGE).unwrap();
        assert_eq!(event_from(&img), None);
        let start: EventMessage = serde_json::from_str(EV_START).unwrap();
        let ev = event_from(&start).unwrap();
        assert_eq!(ev.kind, ResourceKind::Container);
        assert_eq!(ev.action, "start");
        assert!(ev.id.starts_with("79f71dcb"));
    }

    #[test]
    fn raw_stats_from_real_sample() {
        let s: ContainerStatsResponse = serde_json::from_str(STATS).unwrap();
        let r = raw_stats_from(&s);
        assert_eq!(r.cpu_total, 22668000);
        assert_eq!(r.precpu_total, 22668000);
        assert_eq!(r.system, 75708860000000);
        assert_eq!(r.presystem, 75692790000000);
        assert_eq!(r.online_cpus, 16);
        assert_eq!(r.mem_usage, 557056);
        assert_eq!(r.mem_inactive_file, 4096);
        assert_eq!(r.mem_limit, 32984338432);
        assert_eq!((r.net_rx, r.net_tx), (3774, 126));
        assert_eq!((r.blk_read, r.blk_write), (188416, 0));
    }

    #[test]
    fn raw_stats_from_empty_is_zero() {
        let r = raw_stats_from(&ContainerStatsResponse::default());
        assert_eq!(r.cpu_total, 0);
        assert_eq!(r.mem_limit, 0);
    }

    #[test]
    fn log_line_splits_timestamp_and_stream() {
        use bollard::container::LogOutput;
        let l = log_line_from(
            LogOutput::StdErr {
                message: "2026-10-29T13:34:10.1Z hello\r\n".into(),
            },
            true,
        );
        assert_eq!(l.stream, LogStream::Stderr);
        assert_eq!(l.timestamp.as_deref(), Some("2026-10-29T13:34:10.1Z"));
        assert_eq!(l.text, "hello");
        let l = log_line_from(
            LogOutput::StdOut {
                message: b"plain \xff text\n".as_slice().into(),
            },
            false,
        );
        assert_eq!(l.stream, LogStream::Stdout);
        assert_eq!(l.timestamp, None);
        assert_eq!(l.text, "plain \u{fffd} text");
    }

    fn status_err(code: u16) -> bollard::errors::Error {
        bollard::errors::Error::DockerResponseServerError {
            status_code: code,
            message: "boom".into(),
        }
    }

    #[test]
    fn api_error_404_is_not_found() {
        assert_eq!(
            api_error_from(status_err(404)),
            DockerError::NotFound("boom".into())
        );
    }

    #[test]
    fn api_error_409_is_conflict() {
        assert_eq!(
            api_error_from(status_err(409)),
            DockerError::Conflict("boom".into())
        );
    }

    #[test]
    fn api_error_other_status_is_api() {
        assert_eq!(
            api_error_from(status_err(500)),
            DockerError::Api {
                status: 500,
                message: "boom".into()
            }
        );
    }

    #[test]
    fn api_error_io_permission_denied() {
        let e = bollard::errors::Error::IOError {
            err: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        assert_eq!(api_error_from(e), DockerError::PermissionDenied);
    }
}
