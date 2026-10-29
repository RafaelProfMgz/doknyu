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

    iced::application(App::default, App::update, App::view)
        .title("doknyu")
        .run()
}
