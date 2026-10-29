use iced::widget::text;
use iced::{Element, Task};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone)]
enum Message {}

#[derive(Default)]
struct App;

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {}
    }

    fn view(&self) -> Element<'_, Message> {
        text("Hello").into()
    }
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    #[cfg(feature = "fake")]
    let _client: std::sync::Arc<dyn doknyu::docker::DockerClient> =
        if std::env::var_os("DOKNYU_FAKE").is_some_and(|v| v == "1") {
            tracing::info!("using FakeClient with sample data");
            std::sync::Arc::new(doknyu::docker::FakeClient::with_sample_data())
        } else {
            doknyu::docker::connect().expect("failed to create Docker client")
        };

    iced::application(App::default, App::update, App::view)
        .title("doknyu")
        .run()
}
