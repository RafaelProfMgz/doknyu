//! Integration test of `doknyu --exec-bridge` against a real Docker daemon. Run with
//! `cargo test -- --ignored`.

use std::io::Write;
use std::process::{Command, Stdio};

use bollard::Docker;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{CreateContainerOptionsBuilder, CreateImageOptionsBuilder};
use futures::StreamExt;

const IMAGE: &str = "alpine:latest";

/// Removes the container on drop, even if the test panics.
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", "-v", &self.0])
            .output();
    }
}

async fn running_alpine(name: &str) {
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
    let body = ContainerCreateBody {
        image: Some(IMAGE.to_string()),
        cmd: Some(vec!["sleep".into(), "300".into()]),
        ..Default::default()
    };
    let opts = CreateContainerOptionsBuilder::new().name(name).build();
    docker
        .create_container(Some(opts), body)
        .await
        .expect("create container");
    docker
        .start_container(
            name,
            None::<bollard::query_parameters::StartContainerOptions>,
        )
        .await
        .expect("start container");
}

#[tokio::test]
#[ignore]
async fn bridge_runs_a_command() {
    let name = format!("doknyu-it-bridge-{:x}", std::process::id());
    let _cleanup = Cleanup(name.clone());
    running_alpine(&name).await;

    let mut child = Command::new(env!("CARGO_BIN_EXE_doknyu"))
        .args(["--exec-bridge", &name])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the bridge");
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"echo ponte-ok; exit\n").unwrap();
    // Keep stdin open until the shell exits on its own: the bridge ends with the exec.
    let output = tokio::task::spawn_blocking(move || {
        let out = child.wait_with_output().unwrap();
        drop(stdin);
        out
    })
    .await
    .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ponte-ok"), "stdout: {stdout:?}");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
