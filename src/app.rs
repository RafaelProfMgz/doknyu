use std::sync::Arc;
use std::time::Duration;

use iced::widget::{center, column, container, row, stack, text};
use iced::{Element, Length, Subscription, Task};
use rust_i18n::t;

use crate::docker::DockerClient;
use crate::error::DockerError;
use crate::model::{ContainerInfo, DockerEvent};
use crate::views::containers::{self, ContainersState};
use crate::views::onboarding;
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
    /// Copies a command to the clipboard (onboarding blocks).
    Copy(String),
    /// Opens a link in the default browser.
    OpenUrl(String),
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
    /// A ping is in flight; further `Retry` messages are ignored until it resolves.
    pinging: bool,
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

/// A daemon that never answers is treated as stopped, so the retry loop can go on.
async fn ping(client: Arc<dyn DockerClient>) -> Result<(), DockerError> {
    tokio::time::timeout(PING_TIMEOUT, client.ping())
        .await
        .unwrap_or(Err(DockerError::NotRunning))
}

async fn list_all(client: Arc<dyn DockerClient>) -> Result<Vec<ContainerInfo>, DockerError> {
    client.list_containers(true).await
}

/// Longest a ping may take before the daemon is considered unreachable.
const PING_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the app checks again while Docker is unreachable.
const RETRY_INTERVAL: Duration = Duration::from_secs(3);

impl App {
    /// Builds the app and starts the first connectivity check.
    pub fn new(client: Arc<dyn DockerClient>) -> (Self, Task<Message>) {
        let mut app = Self {
            client,
            conn: Connection::Checking,
            screen: Screen::Containers,
            containers: ContainersState::default(),
            toasts: Vec::new(),
            pinging: false,
        };
        let task = app.start_ping();
        (app, task)
    }

    /// Pings the daemon unless a ping is already in flight.
    fn start_ping(&mut self) -> Task<Message> {
        if self.pinging {
            return Task::none();
        }
        self.pinging = true;
        Task::perform(ping(self.client.clone()), Message::Pinged)
    }

    pub fn update(&mut self, m: Message) -> Task<Message> {
        if matches!(m, Message::Pinged(_)) {
            self.pinging = false;
        }
        match m {
            Message::Navigate(screen) => {
                self.screen = screen;
                Task::none()
            }
            Message::Pinged(Ok(())) => {
                self.conn = Connection::Ok;
                self.load_containers()
            }
            Message::Pinged(Err(e)) if e.is_connection() => {
                self.conn = Connection::Failed(e);
                Task::none()
            }
            Message::Pinged(Err(e)) => {
                // The daemon answered, just not with a pong: report it and carry on.
                self.push_toast(friendly_error(&e), Some(format!("{e:?}")));
                self.conn = Connection::Ok;
                self.load_containers()
            }
            Message::Retry => self.start_ping(),
            Message::Copy(text) => iced::clipboard::write(text),
            Message::OpenUrl(url) => {
                if let Err(e) = open::that_detached(&url) {
                    self.push_toast(t!("error.open_link").into_owned(), Some(e.to_string()));
                }
                Task::none()
            }
            Message::DismissToast(i) => {
                if i < self.toasts.len() {
                    self.toasts.remove(i);
                }
                Task::none()
            }
            // Filled in by tasks 2.3 (Containers) and 2.4 (Docker, EventsEnded); Tick is reserved.
            Message::Containers(containers::Msg::Loaded(Ok(items))) => {
                self.containers.items = items;
                Task::none()
            }
            Message::Containers(containers::Msg::Loaded(Err(e))) if e.is_connection() => {
                self.conn = Connection::Failed(e);
                Task::none()
            }
            Message::Containers(containers::Msg::Loaded(Err(e))) => {
                self.push_toast(friendly_error(&e), Some(format!("{e:?}")));
                Task::none()
            }
            Message::Docker(_) | Message::EventsEnded | Message::Tick => Task::none(),
        }
    }

    fn load_containers(&self) -> Task<Message> {
        Task::perform(list_all(self.client.clone()), |r| {
            Message::Containers(containers::Msg::Loaded(r))
        })
    }

    pub fn push_toast(&mut self, text: String, details: Option<String>) {
        self.toasts.push(Toast { text, details });
    }

    pub fn view(&self) -> Element<'_, Message> {
        let body: Element<'_, Message> = match &self.conn {
            Connection::Checking => center(text(t!("connection.checking").into_owned())).into(),
            Connection::Failed(e) => onboarding::view(e),
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

    /// While Docker is unreachable, ping again every few seconds.
    pub fn subscription(&self) -> Subscription<Message> {
        match self.conn {
            Connection::Failed(_) => iced::time::every(RETRY_INTERVAL).map(|_| Message::Retry),
            _ => Subscription::none(),
        }
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
    use crate::docker::{DockerStream, FakeClient};
    use crate::model::{LogLine, NetworkInfo, RawStats, VolumeInfo};

    /// An app with no ping in flight (the initial ping task is discarded).
    fn app_with(fake: &Arc<FakeClient>) -> App {
        let mut app = App::new(fake.clone()).0;
        app.pinging = false;
        app
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

    #[tokio::test]
    async fn ping_failure_shows_onboarding() {
        let fake = Arc::new(FakeClient::new());
        fake.set_ping_error(Some(DockerError::NotRunning));
        let (mut app, task) = App::new(fake.clone());
        for m in run(task).await {
            let _ = app.update(m);
        }
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(app.toasts.is_empty());
    }

    #[tokio::test]
    async fn retry_recovers() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.set_ping_error(Some(DockerError::NotRunning));
        let mut app = app_with(&fake);
        settle(&mut app, Message::Retry).await;
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(app.containers.items.is_empty());

        fake.set_ping_error(None);
        settle(&mut app, Message::Retry).await;
        assert_eq!(app.conn, Connection::Ok);
        assert!(fake.calls().contains(&"list_containers:true".to_string()));
        assert!(!app.containers.items.is_empty());
    }

    #[tokio::test]
    async fn non_connection_ping_error_toasts_and_loads_anyway() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        let e = DockerError::Other("boom".into());
        settle(&mut app, Message::Pinged(Err(e))).await;
        assert_eq!(app.conn, Connection::Ok);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].details.as_deref(), Some("Other(\"boom\")"));
        assert!(fake.calls().contains(&"list_containers:true".to_string()));
    }

    #[tokio::test]
    async fn connection_lost_while_loading_returns_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        let _ = app.update(Message::Pinged(Ok(())));
        let _ = app.update(Message::Containers(containers::Msg::Loaded(Err(
            DockerError::NotRunning,
        ))));
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
    }

    #[test]
    fn retry_tick_runs_only_while_failed() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        for conn in [Connection::Checking, Connection::Ok] {
            app.conn = conn;
            assert_eq!(app.subscription().units(), 0);
        }
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert_eq!(app.subscription().units(), 1);
    }

    #[tokio::test]
    async fn retry_while_pinging_is_ignored() {
        let fake = Arc::new(FakeClient::new());
        let (mut app, first) = App::new(fake.clone());
        // The initial ping is still in flight: a retry must not start another one.
        let second = app.update(Message::Retry);
        assert!(run(second).await.is_empty());
        let produced = run(first).await;
        assert_eq!(produced.len(), 1);
        assert_eq!(fake.calls(), vec!["ping"]);

        // Once the ping resolves, retries work again, and again only one at a time.
        for m in produced {
            let _ = app.update(m);
        }
        let a = app.update(Message::Retry);
        let b = app.update(Message::Retry);
        assert_eq!(run(a).await.len(), 1);
        assert!(run(b).await.is_empty());
        assert_eq!(fake.calls().iter().filter(|c| *c == "ping").count(), 2);
    }

    /// A daemon that accepts the connection but never answers.
    #[derive(Default)]
    struct HungClient {
        pings: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl DockerClient for HungClient {
        async fn ping(&self) -> Result<(), DockerError> {
            self.pings.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::pending().await
        }
        async fn list_containers(&self, _all: bool) -> Result<Vec<ContainerInfo>, DockerError> {
            std::future::pending().await
        }
        async fn start(&self, _id: &str) -> Result<(), DockerError> {
            std::future::pending().await
        }
        async fn stop(&self, _id: &str) -> Result<(), DockerError> {
            std::future::pending().await
        }
        async fn restart(&self, _id: &str) -> Result<(), DockerError> {
            std::future::pending().await
        }
        async fn remove(&self, _id: &str, _force: bool) -> Result<(), DockerError> {
            std::future::pending().await
        }
        fn logs(&self, _id: &str, _tail: usize) -> DockerStream<LogLine> {
            futures::StreamExt::boxed(futures::stream::pending())
        }
        fn stats(&self, _id: &str) -> DockerStream<RawStats> {
            futures::StreamExt::boxed(futures::stream::pending())
        }
        fn events(&self) -> DockerStream<DockerEvent> {
            futures::StreamExt::boxed(futures::stream::pending())
        }
        async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError> {
            std::future::pending().await
        }
        async fn remove_volume(&self, _name: &str) -> Result<(), DockerError> {
            std::future::pending().await
        }
        async fn prune_volumes(&self) -> Result<Vec<String>, DockerError> {
            std::future::pending().await
        }
        async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError> {
            std::future::pending().await
        }
        async fn remove_network(&self, _id: &str) -> Result<(), DockerError> {
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn hung_ping_times_out_and_allows_retry() {
        let hung = Arc::new(HungClient::default());
        let pings = || hung.pings.load(std::sync::atomic::Ordering::SeqCst);
        let (mut app, task) = App::new(hung.clone());
        // Outer guard so a missing timeout fails the test instead of hanging it.
        let produced = tokio::time::timeout(Duration::from_secs(60), run(task))
            .await
            .expect("ping never timed out");
        assert!(matches!(
            produced[..],
            [Message::Pinged(Err(DockerError::NotRunning))]
        ));
        for m in produced {
            let _ = app.update(m);
        }
        assert!(!app.pinging);
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert_eq!(pings(), 1);

        let retry = app.update(Message::Retry);
        let produced = tokio::time::timeout(Duration::from_secs(60), run(retry))
            .await
            .expect("retry ping never timed out");
        assert_eq!(produced.len(), 1);
        assert_eq!(pings(), 2);
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
