use std::sync::Arc;

use doknyu::app::{App, normalize_locale};
use doknyu::docker::DockerClient;
use tracing_subscriber::EnvFilter;

/// Client used when `docker::connect()` itself fails: every call reports the error, so the app
/// still opens and the onboarding screen explains the problem.
#[cfg(not(feature = "fake"))]
struct Unavailable(doknyu::error::DockerError);

#[cfg(not(feature = "fake"))]
mod unavailable {
    use futures::StreamExt;
    use futures::stream;

    use super::Unavailable;
    use doknyu::docker::{DockerClient, DockerStream};
    use doknyu::error::DockerError;
    use doknyu::model::*;

    impl Unavailable {
        fn err<T>(&self) -> Result<T, DockerError> {
            Err(self.0.clone())
        }

        fn failing<T: Send + 'static>(&self) -> DockerStream<T> {
            stream::once(std::future::ready(Err(self.0.clone()))).boxed()
        }
    }

    #[async_trait::async_trait]
    impl DockerClient for Unavailable {
        async fn ping(&self) -> Result<(), DockerError> {
            self.err()
        }
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>, DockerError> {
            self.err()
        }
        async fn start(&self, _id: &str) -> Result<(), DockerError> {
            self.err()
        }
        async fn stop(&self, _id: &str) -> Result<(), DockerError> {
            self.err()
        }
        async fn restart(&self, _id: &str) -> Result<(), DockerError> {
            self.err()
        }
        async fn remove(&self, _id: &str, _force: bool) -> Result<(), DockerError> {
            self.err()
        }
        fn logs(&self, _id: &str, _tail: usize) -> DockerStream<LogLine> {
            self.failing()
        }
        fn stats(&self, _id: &str) -> DockerStream<RawStats> {
            self.failing()
        }
        fn events(&self) -> DockerStream<DockerEvent> {
            self.failing()
        }
        async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError> {
            self.err()
        }
        async fn remove_volume(&self, _name: &str) -> Result<(), DockerError> {
            self.err()
        }
        async fn prune_volumes(&self) -> Result<Vec<String>, DockerError> {
            self.err()
        }
        async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError> {
            self.err()
        }
        async fn remove_network(&self, _id: &str) -> Result<(), DockerError> {
            self.err()
        }
    }
}

#[cfg(feature = "fake")]
fn make_client() -> Arc<dyn DockerClient> {
    Arc::new(doknyu::docker::FakeClient::with_sample_data())
}

#[cfg(not(feature = "fake"))]
fn make_client() -> Arc<dyn DockerClient> {
    match doknyu::docker::connect() {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("could not create the Docker client: {e}");
            Arc::new(Unavailable(e))
        }
    }
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let locale = sys_locale::get_locale().unwrap_or_default();
    rust_i18n::set_locale(normalize_locale(&locale));

    let client = make_client();
    iced::application(move || App::new(client.clone()), App::update, App::view)
        .subscription(App::subscription)
        .title("doknyu")
        .run()
}
