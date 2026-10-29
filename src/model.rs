//! Domain types shared by the core and the UI. No dependency on bollard.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerState {
    Running,
    Paused,
    Restarting,
    Created,
    Exited { code: i64 },
    Dead,
}

impl ContainerState {
    pub fn is_running(&self) -> bool {
        matches!(self, ContainerState::Running)
    }

    /// `Dead` or `Exited` with a non-zero code.
    pub fn is_error(&self) -> bool {
        match self {
            ContainerState::Dead => true,
            ContainerState::Exited { code } => *code != 0,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortMapping {
    pub private: u16,
    pub public: Option<u16>,
    pub protocol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    pub status_text: String,
    pub ports: Vec<PortMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStats {
    pub cpu_total: u64,
    pub precpu_total: u64,
    pub system: u64,
    pub presystem: u64,
    pub online_cpus: u32,
    pub mem_usage: u64,
    pub mem_inactive_file: u64,
    pub mem_limit: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub blk_read: u64,
    pub blk_write: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatsSample {
    pub cpu_percent: f64,
    pub mem_used: u64,
    pub mem_limit: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub blk_read: u64,
    pub blk_write: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub stream: LogStream,
    pub timestamp: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub used_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub containers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Container,
    Volume,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerEvent {
    pub kind: ResourceKind,
    pub action: String,
    pub id: String,
}
