//! Integration tests against a real Docker daemon. Run with `cargo test -- --ignored`.

use std::process::Command;
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig, VolumeCreateRequest};
use bollard::query_parameters::{CreateContainerOptionsBuilder, CreateImageOptionsBuilder};
use doknyu::docker::{BollardClient, DockerClient, connect};
use doknyu::error::DockerError;
use doknyu::model::{ContainerState, ResourceKind};
use futures::StreamExt;
use tokio::time::timeout;

const IMAGE: &str = "alpine:latest";

fn unique(kind: &str) -> String {
    let id = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("doknyu-it-{kind}-{id:x}{nanos:x}{seq}")
}

/// Removes the named container and volume on drop, even if the test panics.
struct Cleanup {
    containers: Vec<String>,
    volumes: Vec<String>,
}

impl Cleanup {
    fn new() -> Self {
        Self {
            containers: vec![],
            volumes: vec![],
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for c in &self.containers {
            let _ = Command::new("docker").args(["rm", "-f", "-v", c]).output();
        }
        for v in &self.volumes {
            let _ = Command::new("docker")
                .args(["volume", "rm", "-f", v])
                .output();
        }
    }
}

async fn raw() -> Docker {
    let docker = Docker::connect_with_unix_defaults().expect("docker socket");
    if docker.inspect_image(IMAGE).await.is_err() {
        let opts = CreateImageOptionsBuilder::new()
            .from_image("alpine")
            .tag("latest")
            .build();
        let mut s = docker.create_image(Some(opts), None, None);
        while let Some(r) = s.next().await {
            r.expect("pull alpine");
        }
    }
    docker
}

async fn create(
    docker: &Docker,
    cleanup: &mut Cleanup,
    cmd: &str,
    binds: Option<Vec<String>>,
) -> String {
    let name = unique("c");
    cleanup.containers.push(name.clone());
    let body = ContainerCreateBody {
        image: Some(IMAGE.to_string()),
        cmd: Some(vec!["sh".into(), "-c".into(), cmd.into()]),
        host_config: binds.map(|b| HostConfig {
            binds: Some(b),
            ..Default::default()
        }),
        ..Default::default()
    };
    let opts = CreateContainerOptionsBuilder::new().name(&name).build();
    docker
        .create_container(Some(opts), body)
        .await
        .expect("create container");
    name
}

#[tokio::test]
#[ignore]
async fn lifecycle() {
    let docker = raw().await;
    let mut cleanup = Cleanup::new();
    let name = create(&docker, &mut cleanup, "echo hello; sleep 300", None).await;
    let client = connect().unwrap();
    client.ping().await.unwrap();

    client.start(&name).await.unwrap();
    let running = client.list_containers(false).await.unwrap();
    let info = running.iter().find(|c| c.name == name).expect("listed");
    assert_eq!(info.state, ContainerState::Running);

    let mut logs = client.logs(&name, 10);
    let found = timeout(Duration::from_secs(5), async {
        while let Some(line) = logs.next().await {
            if line.unwrap().text == "hello" {
                return true;
            }
        }
        false
    })
    .await
    .expect("log timeout");
    assert!(found, "hello line not found");

    let mut stats = client.stats(&name);
    for _ in 0..2 {
        timeout(Duration::from_secs(10), stats.next())
            .await
            .expect("stats timeout")
            .expect("stats ended")
            .unwrap();
    }

    client.stop(&name).await.unwrap();
    let all = client.list_containers(true).await.unwrap();
    let info = all.iter().find(|c| c.name == name).expect("still listed");
    assert!(matches!(info.state, ContainerState::Exited { .. }));

    client.remove(&name, false).await.unwrap();
    let all = client.list_containers(true).await.unwrap();
    assert!(all.iter().all(|c| c.name != name));
}

#[tokio::test]
#[ignore]
async fn remove_missing_is_not_found() {
    let client = connect().unwrap();
    let err = client
        .remove("doknyu-it-nao-existe", false)
        .await
        .unwrap_err();
    assert!(matches!(err, DockerError::NotFound(_)), "got {err:?}");
}

#[tokio::test]
#[ignore]
async fn events_see_start() {
    let docker = raw().await;
    let mut cleanup = Cleanup::new();
    let name = create(&docker, &mut cleanup, "sleep 300", None).await;
    let client = connect().unwrap();

    // Streams are lazy: drive it in a task so the subscription is live before `start`.
    let mut events = client.events();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            if tx.send(ev).is_err() {
                break;
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    client.start(&name).await.unwrap();

    let seen = timeout(Duration::from_secs(5), async {
        while let Some(ev) = rx.recv().await {
            let ev = ev.unwrap();
            if ev.kind == ResourceKind::Container && ev.action == "start" {
                return true;
            }
        }
        false
    })
    .await
    .expect("event timeout");
    task.abort();
    assert!(seen);
}

#[tokio::test]
#[ignore]
async fn volume_in_use_conflict() {
    let docker = raw().await;
    let mut cleanup = Cleanup::new();
    let vol = unique("v");
    cleanup.volumes.push(vol.clone());
    docker
        .create_volume(VolumeCreateRequest {
            name: Some(vol.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    let name = create(
        &docker,
        &mut cleanup,
        "sleep 300",
        Some(vec![format!("{vol}:/data")]),
    )
    .await;
    let client = connect().unwrap();
    client.start(&name).await.unwrap();

    let vols = client.list_volumes().await.unwrap();
    let info = vols.iter().find(|v| v.name == vol).expect("volume listed");
    assert_eq!(info.used_by, vec![name.clone()]);

    let err = client.remove_volume(&vol).await.unwrap_err();
    assert!(matches!(err, DockerError::Conflict(_)), "got {err:?}");
}

#[tokio::test]
#[ignore]
async fn start_running_is_ok() {
    let docker = raw().await;
    let mut cleanup = Cleanup::new();
    let name = create(&docker, &mut cleanup, "sleep 300", None).await;
    let client = connect().unwrap();
    client.start(&name).await.unwrap();
    client.start(&name).await.unwrap();
    client.stop(&name).await.unwrap();
    client.stop(&name).await.unwrap();
}

/// Temporary directory removed on drop (sockets live inside it).
struct TmpDir(std::path::PathBuf);

impl TmpDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(unique("sock"));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn sock(&self) -> String {
        self.0.join("docker.sock").to_string_lossy().into_owned()
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
#[ignore]
async fn missing_socket_is_not_running_or_not_installed() {
    let tmp = TmpDir::new();
    let client = BollardClient::with_socket(tmp.sock());
    let cli = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("docker").is_file()));
    let expected = if cli {
        DockerError::NotRunning
    } else {
        DockerError::NotInstalled
    };
    assert_eq!(client.ping().await, Err(expected));
}

#[tokio::test]
#[ignore]
async fn orphan_socket_file_is_not_running() {
    let tmp = TmpDir::new();
    let path = tmp.sock();
    // Bind then drop: the socket file stays behind with nobody listening.
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(std::path::Path::new(&path).exists());
    let client = BollardClient::with_socket(path);
    assert_eq!(client.ping().await, Err(DockerError::NotRunning));
    let mut events = client.events();
    assert_eq!(events.next().await, Some(Err(DockerError::NotRunning)));
}

#[tokio::test]
#[ignore]
async fn unreadable_socket_is_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TmpDir::new();
    let path = tmp.sock();
    let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let client = BollardClient::with_socket(path);
    assert_eq!(client.ping().await, Err(DockerError::PermissionDenied));
}
