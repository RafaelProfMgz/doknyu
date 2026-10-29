use std::sync::Arc;

use iced::widget::{center, column, container, row, stack, text};
use iced::{Element, Length, Subscription, Task};
use rust_i18n::t;

use crate::docker::DockerClient;
use crate::error::DockerError;
use crate::model::DockerEvent;
use crate::views::containers::{self, ContainersState};
use crate::views::sidebar;
use crate::views::widgets::{friendly_error, toast_stack};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Containers,
    Volumes,
    Networks,
    Detail(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Connection {
    Checking,
    Ok,
    Failed(DockerError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Screen),
    Pinged(Result<(), DockerError>),
    Retry,
    DismissToast(usize),
    Containers(containers::Msg),
    Docker(DockerEvent),
    EventsEnded,
    Tick,
}

pub struct App {
    client: Arc<dyn DockerClient>,
    pub conn: Connection,
    pub screen: Screen,
    pub containers: ContainersState,
    pub toasts: Vec<Toast>,
}

/// Maps a system locale (`pt_BR.UTF-8`, `en_US`, `fr`...) to a supported locale.
pub fn normalize_locale(raw: &str) -> &'static str {
    let lang = raw.split(['_', '-', '.', '@']).next().unwrap_or("");
    if lang.eq_ignore_ascii_case("pt") {
        "pt-BR"
    } else {
        "en"
    }
}

async fn ping(client: Arc<dyn DockerClient>) -> Result<(), DockerError> {
    client.ping().await
}

impl App {
    /// Builds the app and starts the first connectivity check.
    pub fn new(client: Arc<dyn DockerClient>) -> (Self, Task<Message>) {
        let task = Task::perform(ping(client.clone()), Message::Pinged);
        let app = Self {
            client,
            conn: Connection::Checking,
            screen: Screen::Containers,
            containers: ContainersState::default(),
            toasts: Vec::new(),
        };
        (app, task)
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Navigate(screen) => {
                self.screen = screen;
                Task::none()
            }
            Message::Pinged(Ok(())) => {
                self.conn = Connection::Ok;
                Task::none()
            }
            Message::Pinged(Err(e)) => {
                self.conn = Connection::Failed(e);
                Task::none()
            }
            Message::Retry => Task::perform(ping(self.client.clone()), Message::Pinged),
            Message::DismissToast(i) => {
                if i < self.toasts.len() {
                    self.toasts.remove(i);
                }
                Task::none()
            }
            // Filled in by tasks 2.3 (Containers) and 2.4 (Docker, EventsEnded); Tick is reserved.
            Message::Containers(msg) => match msg {},
            Message::Docker(_) | Message::EventsEnded | Message::Tick => Task::none(),
        }
    }

    pub fn push_toast(&mut self, text: String, details: Option<String>) {
        self.toasts.push(Toast { text, details });
    }

    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.conn {
            Connection::Checking => center(text(t!("connection.checking").into_owned())).into(),
            Connection::Failed(e) => center(
                column![
                    text(t!("connection.failed").into_owned()).size(24),
                    text(friendly_error(e)),
                ]
                .spacing(8),
            )
            .into(),
            Connection::Ok => row![sidebar::view(&self.screen), self.main_area()].into(),
        };
        if self.toasts.is_empty() {
            body
        } else {
            stack![body, toast_stack(&self.toasts)].into()
        }
    }

    fn main_area(&self) -> Element<'_, Message> {
        let title = match &self.screen {
            Screen::Containers => t!("sidebar.containers"),
            Screen::Volumes => t!("sidebar.volumes"),
            Screen::Networks => t!("sidebar.networks"),
            Screen::Detail(_) => t!("detail.title"),
        };
        container(
            column![
                text(title.into_owned()).size(24),
                text(t!("common.coming_soon").into_owned()),
            ]
            .spacing(8),
        )
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }
}

/// Helpers to drive `App::update` in tests. Convention for tasks 2.2-2.4:
///
/// * `run(task)` executes the real `Task` returned by `update` (its futures and streams) on the
///   tokio test runtime and collects the messages it produces. Non-message actions (widget
///   operations, window or clipboard actions) are ignored.
/// * `settle(app, msg)` calls `update(msg)`, runs the task and feeds the produced messages back
///   into `update`, repeating until nothing is left, like the iced runtime would. Use
///   `#[tokio::test]` (with `start_paused = true` for debounce and timers).
/// * Never `settle` a task that does not finish (e.g. a live event stream); drive those with
///   `run`/`update` and the `FakeClient` controls instead.
#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::VecDeque;

    use futures::StreamExt;
    use iced_runtime::Action;
    use iced_runtime::task::into_stream;

    use super::*;

    /// Runs a `Task` to completion and returns every message it produced.
    pub async fn run<T: 'static>(task: Task<T>) -> Vec<T> {
        match into_stream(task) {
            None => Vec::new(),
            Some(stream) => {
                stream
                    .filter_map(|action| async move {
                        match action {
                            Action::Output(message) => Some(message),
                            _ => None,
                        }
                    })
                    .collect()
                    .await
            }
        }
    }

    /// Sends `msg` to `app.update`, then feeds every message produced by the returned tasks back
    /// into `update` until none are left. Returns every message handled, in order.
    pub async fn settle(app: &mut App, msg: Message) -> Vec<Message> {
        let mut queue = VecDeque::from([msg]);
        let mut handled = Vec::new();
        while let Some(m) = queue.pop_front() {
            handled.push(m.clone());
            queue.extend(run(app.update(m)).await);
        }
        handled
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{run, settle};
    use super::*;
    use crate::docker::FakeClient;

    fn app_with(fake: &Arc<FakeClient>) -> App {
        App::new(fake.clone()).0
    }

    #[test]
    fn navigate_changes_screen() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        assert_eq!(app.screen, Screen::Containers);
        let _ = app.update(Message::Navigate(Screen::Volumes));
        assert_eq!(app.screen, Screen::Volumes);
        let _ = app.update(Message::Navigate(Screen::Detail("web-id".into())));
        assert_eq!(app.screen, Screen::Detail("web-id".into()));
    }

    #[test]
    fn friendly_error_conflict_is_human() {
        let e = DockerError::Conflict("409 Client Error: container is running".into());
        let text = friendly_error(&e);
        assert!(!text.contains("409"), "{text}");
        assert!(!text.starts_with("error."), "missing translation: {text}");
    }

    #[test]
    fn friendly_error_covers_every_variant() {
        let all = [
            DockerError::NotInstalled,
            DockerError::NotRunning,
            DockerError::PermissionDenied,
            DockerError::NotFound("x".into()),
            DockerError::Conflict("x".into()),
            DockerError::Api {
                status: 500,
                message: "x".into(),
            },
            DockerError::Other("x".into()),
        ];
        for e in all {
            let text = friendly_error(&e);
            assert!(
                !text.is_empty() && !text.starts_with("error."),
                "{e:?} -> {text}"
            );
        }
    }

    #[test]
    fn locale_normalization() {
        assert_eq!(normalize_locale("pt_BR.UTF-8"), "pt-BR");
        assert_eq!(normalize_locale("pt-BR"), "pt-BR");
        assert_eq!(normalize_locale("pt"), "pt-BR");
        assert_eq!(normalize_locale("en_US"), "en");
        assert_eq!(normalize_locale("fr_FR"), "en");
        assert_eq!(normalize_locale(""), "en");
    }

    #[test]
    fn locales_have_the_same_keys() {
        fn keys(src: &str) -> Vec<String> {
            let mut k: Vec<String> = src
                .lines()
                .filter(|l| !l.starts_with(['#', ' ']) && l.contains(':'))
                .map(|l| l.split(':').next().unwrap().to_string())
                .collect();
            k.sort();
            k
        }
        let en = keys(include_str!("../locales/en.yml"));
        let pt = keys(include_str!("../locales/pt-BR.yml"));
        assert!(en.len() > 10);
        assert_eq!(en, pt);
    }

    #[tokio::test]
    async fn new_pings_the_daemon_and_marks_connection_ok() {
        let fake = Arc::new(FakeClient::new());
        let (mut app, task) = App::new(fake.clone());
        assert_eq!(app.conn, Connection::Checking);
        let produced = run(task).await;
        assert_eq!(produced.len(), 1);
        for m in produced {
            let _ = app.update(m);
        }
        assert_eq!(app.conn, Connection::Ok);
        assert_eq!(fake.calls(), vec!["ping"]);
    }

    #[tokio::test]
    async fn failed_ping_marks_connection_failed() {
        let fake = Arc::new(FakeClient::new());
        fake.set_ping_error(Some(DockerError::NotRunning));
        let mut app = app_with(&fake);
        let handled = settle(&mut app, Message::Retry).await;
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(matches!(
            handled[..],
            [Message::Retry, Message::Pinged(Err(_))]
        ));
    }

    #[test]
    fn dismiss_toast_removes_only_that_toast() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.push_toast("a".into(), None);
        app.push_toast("b".into(), Some("detail".into()));
        let _ = app.update(Message::DismissToast(0));
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].text, "b");
        let _ = app.update(Message::DismissToast(9)); // out of range: ignored
        assert_eq!(app.toasts.len(), 1);
    }

    #[test]
    fn view_builds_for_every_screen_and_connection_state() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.push_toast("a".into(), Some("detail".into()));
        for conn in [
            Connection::Checking,
            Connection::Failed(DockerError::NotRunning),
            Connection::Ok,
        ] {
            app.conn = conn;
            for screen in [
                Screen::Containers,
                Screen::Volumes,
                Screen::Networks,
                Screen::Detail("web-id".into()),
            ] {
                app.screen = screen;
                let _ = app.view();
            }
        }
        let _ = app.subscription();
    }
}
