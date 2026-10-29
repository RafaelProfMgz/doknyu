use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use iced::widget::{center, column, container, row, stack, text};
use iced::{Element, Length, Subscription, Task};
use rust_i18n::t;

use crate::docker::DockerClient;
use crate::error::DockerError;
use crate::model::{ContainerInfo, DockerEvent, ResourceKind};
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
    /// The technical details are shown.
    pub expanded: bool,
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
    /// Shows or hides the technical details of a toast.
    ToggleToast(usize),
    Containers(containers::Msg),
    Docker(DockerEvent),
    EventsEnded,
    /// The debounce window after a container event is over: reload the list.
    ReloadContainers,
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
    /// Bumped on every successful ping so each (re)connection gets a fresh events subscription.
    events_generation: u64,
    /// A reload is waiting for the debounce window to end; further events join it.
    reload_scheduled: bool,
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

#[derive(Debug, Clone, Copy)]
enum ContainerAction {
    Start,
    Stop,
    Restart,
    Remove { force: bool },
}

async fn run_action(
    client: Arc<dyn DockerClient>,
    id: String,
    action: ContainerAction,
) -> Result<(), DockerError> {
    match action {
        ContainerAction::Start => client.start(&id).await,
        ContainerAction::Stop => client.stop(&id).await,
        ContainerAction::Restart => client.restart(&id).await,
        ContainerAction::Remove { force } => client.remove(&id, force).await,
    }
}

/// Longest a ping may take before the daemon is considered unreachable.
const PING_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the app checks again while Docker is unreachable.
const RETRY_INTERVAL: Duration = Duration::from_secs(3);

/// Container events closer together than this share a single list reload.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(300);

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
            events_generation: 0,
            reload_scheduled: false,
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
                self.events_generation += 1;
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
                self.events_generation += 1;
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
            Message::ToggleToast(i) => {
                if let Some(toast) = self.toasts.get_mut(i) {
                    toast.expanded = !toast.expanded;
                }
                Task::none()
            }
            Message::Containers(m) => self.update_containers(m),
            Message::Docker(ev) => self.on_docker_event(ev),
            Message::ReloadContainers => {
                self.reload_scheduled = false;
                if self.conn == Connection::Ok {
                    self.load_containers()
                } else {
                    Task::none()
                }
            }
            // The stream is gone: either the daemon stopped (the ping fails and the onboarding
            // takes over) or it is still there and the new ping renews the subscription.
            Message::EventsEnded => self.start_ping(),
            // Reserved.
            Message::Tick => Task::none(),
        }
    }

    fn update_containers(&mut self, m: containers::Msg) -> Task<Message> {
        use containers::Msg;
        match m {
            Msg::Loaded(Ok(items)) => {
                self.containers.items = items;
                Task::none()
            }
            Msg::Loaded(Err(e)) => self.fail(e),
            Msg::Query(query) => {
                self.containers.query = query;
                Task::none()
            }
            Msg::ToggleStopped(show) => {
                self.containers.show_stopped = show;
                Task::none()
            }
            Msg::Start(id) => self.container_action(id, ContainerAction::Start),
            Msg::Stop(id) => self.container_action(id, ContainerAction::Stop),
            Msg::Restart(id) => self.container_action(id, ContainerAction::Restart),
            Msg::AskRemove(id) => {
                self.containers.confirm_remove = Some(id);
                Task::none()
            }
            Msg::ConfirmRemove { force } => match self.containers.confirm_remove.take() {
                Some(id) => self.container_action(id, ContainerAction::Remove { force }),
                None => Task::none(),
            },
            Msg::CancelRemove => {
                self.containers.confirm_remove = None;
                Task::none()
            }
            // The list is refreshed by the Docker events the action causes (task 2.4).
            Msg::ActionDone(id, result) => {
                self.containers.pending.remove(&id);
                match result {
                    Ok(()) => Task::none(),
                    Err(e) => self.fail(e),
                }
            }
            Msg::OpenPort(port) => {
                self.update(Message::OpenUrl(format!("http://localhost:{port}")))
            }
            Msg::Open(id) => self.update(Message::Navigate(Screen::Detail(id))),
        }
    }

    /// Container events refresh the list, debounced so a burst causes a single reload.
    /// Volume and network events are handled in phase 4.
    fn on_docker_event(&mut self, ev: DockerEvent) -> Task<Message> {
        if ev.kind != ResourceKind::Container || self.reload_scheduled {
            return Task::none();
        }
        self.reload_scheduled = true;
        Task::perform(tokio::time::sleep(RELOAD_DEBOUNCE), |()| {
            Message::ReloadContainers
        })
    }

    /// Connection problems send the user to the onboarding; anything else becomes a toast.
    fn fail(&mut self, e: DockerError) -> Task<Message> {
        if e.is_connection() {
            self.conn = Connection::Failed(e);
        } else {
            self.push_toast(friendly_error(&e), Some(format!("{e:?}")));
        }
        Task::none()
    }

    /// Runs a container action unless one is already running for that container.
    fn container_action(&mut self, id: String, action: ContainerAction) -> Task<Message> {
        if !self.containers.pending.insert(id.clone()) {
            return Task::none();
        }
        let client = self.client.clone();
        Task::perform(run_action(client, id.clone(), action), move |r| {
            Message::Containers(containers::Msg::ActionDone(id.clone(), r))
        })
    }

    fn load_containers(&self) -> Task<Message> {
        Task::perform(list_all(self.client.clone()), |r| {
            Message::Containers(containers::Msg::Loaded(r))
        })
    }

    pub fn push_toast(&mut self, text: String, details: Option<String>) {
        self.toasts.push(Toast {
            text,
            details,
            expanded: false,
        });
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
        if self.screen == Screen::Containers {
            return containers::view(&self.containers);
        }
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

    /// While Docker is reachable, listen to its events; while it is not, ping again every few
    /// seconds.
    pub fn subscription(&self) -> Subscription<Message> {
        match self.conn {
            Connection::Ok => Subscription::run_with(EventsSource::new(self), |source| {
                event_messages(source.client.clone())
            }),
            Connection::Failed(_) => iced::time::every(RETRY_INTERVAL).map(|_| Message::Retry),
            Connection::Checking => Subscription::none(),
        }
    }
}

/// Identity of the events subscription: a new generation makes iced start a new stream.
struct EventsSource {
    client: Arc<dyn DockerClient>,
    generation: u64,
}

impl EventsSource {
    fn new(app: &App) -> Self {
        Self {
            client: app.client.clone(),
            generation: app.events_generation,
        }
    }
}

impl Hash for EventsSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
    }
}

/// Docker events as messages. When the stream fails or ends, yields `EventsEnded` and stops.
fn event_messages(client: Arc<dyn DockerClient>) -> impl Stream<Item = Message> {
    client
        .events()
        .take_while(|event| std::future::ready(event.is_ok()))
        .filter_map(|event| std::future::ready(event.ok().map(Message::Docker)))
        .chain(futures::stream::once(std::future::ready(
            Message::EventsEnded,
        )))
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
    use crate::model::{ContainerState, LogLine, NetworkInfo, RawStats, ResourceKind, VolumeInfo};

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
        app.conn = Connection::Checking;
        assert_eq!(app.subscription().units(), 0);
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

    /// An app already connected, with the sample containers loaded.
    async fn loaded_app(fake: &Arc<FakeClient>) -> App {
        let mut app = app_with(fake);
        settle(&mut app, Message::Pinged(Ok(()))).await;
        assert_eq!(app.containers.items.len(), 3);
        app
    }

    #[tokio::test]
    async fn stop_calls_client_and_marks_pending() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;

        let task = app.update(Message::Containers(containers::Msg::Stop("web-id".into())));
        assert!(app.containers.pending.contains("web-id"));
        let produced = run(task).await;
        assert!(fake.calls().contains(&"stop:web-id".to_string()));
        assert!(app.containers.pending.contains("web-id"));

        for m in produced {
            let _ = app.update(m);
        }
        assert!(app.containers.pending.is_empty());
        assert!(app.toasts.is_empty());
    }

    #[tokio::test]
    async fn start_and_restart_call_the_client() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        settle(
            &mut app,
            Message::Containers(containers::Msg::Start("worker-id".into())),
        )
        .await;
        settle(
            &mut app,
            Message::Containers(containers::Msg::Restart("db-id".into())),
        )
        .await;
        let calls = fake.calls();
        assert!(calls.contains(&"start:worker-id".to_string()), "{calls:?}");
        assert!(calls.contains(&"restart:db-id".to_string()), "{calls:?}");
        assert!(app.containers.pending.is_empty());
    }

    #[tokio::test]
    async fn action_does_not_reload_the_list() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = fake.calls().len();
        settle(
            &mut app,
            Message::Containers(containers::Msg::Stop("web-id".into())),
        )
        .await;
        assert_eq!(fake.calls()[before..], ["stop:web-id".to_string()]);
    }

    #[tokio::test]
    async fn action_is_ignored_while_the_container_is_pending() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let first = app.update(Message::Containers(containers::Msg::Stop("web-id".into())));
        let second = app.update(Message::Containers(containers::Msg::Restart(
            "web-id".into(),
        )));
        assert_eq!(run(first).await.len(), 1);
        assert!(run(second).await.is_empty());
        assert!(!fake.calls().contains(&"restart:web-id".to_string()));
    }

    #[tokio::test]
    async fn remove_running_requires_force_dialog() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;

        let task = app.update(Message::Containers(containers::Msg::AskRemove(
            "web-id".into(),
        )));
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("web-id"));
        assert!(run(task).await.is_empty());
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove")));

        settle(
            &mut app,
            Message::Containers(containers::Msg::ConfirmRemove { force: true }),
        )
        .await;
        assert!(app.containers.confirm_remove.is_none());
        assert!(app.containers.pending.is_empty());
        assert!(fake.calls().contains(&"remove:web-id:force".to_string()));
    }

    #[tokio::test]
    async fn remove_stopped_uses_plain_remove() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "worker-id".into(),
        )));
        settle(
            &mut app,
            Message::Containers(containers::Msg::ConfirmRemove { force: false }),
        )
        .await;
        assert!(fake.calls().contains(&"remove:worker-id".to_string()));
    }

    #[test]
    fn cancel_remove_closes_the_dialog() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "web-id".into(),
        )));
        let _ = app.update(Message::Containers(containers::Msg::CancelRemove));
        assert!(app.containers.confirm_remove.is_none());
    }

    #[tokio::test]
    async fn confirm_remove_without_dialog_does_nothing() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let task = app.update(Message::Containers(containers::Msg::ConfirmRemove {
            force: true,
        }));
        assert!(run(task).await.is_empty());
        assert!(app.containers.pending.is_empty());
    }

    #[tokio::test]
    async fn action_error_creates_toast() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        fake.fail_next(DockerError::Conflict("409 in use".into()));
        settle(
            &mut app,
            Message::Containers(containers::Msg::Stop("web-id".into())),
        )
        .await;
        assert_eq!(app.toasts.len(), 1);
        assert!(!app.toasts[0].text.contains("409"));
        assert_eq!(
            app.toasts[0].details.as_deref(),
            Some("Conflict(\"409 in use\")")
        );
        assert!(app.containers.pending.is_empty());
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn action_connection_error_goes_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        fake.fail_next(DockerError::NotRunning);
        settle(
            &mut app,
            Message::Containers(containers::Msg::Stop("web-id".into())),
        )
        .await;
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(app.toasts.is_empty());
        assert!(app.containers.pending.is_empty());
    }

    #[test]
    fn query_and_toggle_update_the_filters() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        let _ = app.update(Message::Containers(containers::Msg::Query("ng".into())));
        assert_eq!(app.containers.query, "ng");
        let _ = app.update(Message::Containers(containers::Msg::ToggleStopped(false)));
        assert!(!app.containers.show_stopped);
    }

    #[test]
    fn open_navigates_to_the_detail() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        let _ = app.update(Message::Containers(containers::Msg::Open("web-id".into())));
        assert_eq!(app.screen, Screen::Detail("web-id".into()));
    }

    #[tokio::test]
    async fn empty_list_shows_empty_state() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        settle(&mut app, Message::Pinged(Ok(()))).await;
        assert!(app.containers.items.is_empty());
        let _ = app.view();
        for locale in ["en", "pt-BR"] {
            assert_ne!(
                t!("containers.empty", locale = locale),
                "containers.empty",
                "missing {locale} translation"
            );
        }
    }

    #[tokio::test]
    async fn containers_view_builds_with_items_dialog_and_pending() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.view();
        app.containers.pending.insert("db-id".into());
        app.containers.confirm_remove = Some("web-id".into());
        let _ = app.view();
        app.containers.confirm_remove = Some("worker-id".into());
        let _ = app.view();
        app.containers.confirm_remove = Some("gone-id".into());
        app.containers.query = "zzz".into();
        let _ = app.view();
    }

    #[test]
    fn toggle_toast_expands_details() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.push_toast("a".into(), Some("detail".into()));
        app.push_toast("b".into(), Some("other".into()));
        assert!(!app.toasts[0].expanded);
        let _ = app.update(Message::ToggleToast(0));
        assert!(app.toasts[0].expanded);
        assert!(!app.toasts[1].expanded);
        let _ = app.update(Message::ToggleToast(0));
        assert!(!app.toasts[0].expanded);
        let _ = app.update(Message::ToggleToast(9)); // out of range: ignored
        let _ = app.view();
    }

    fn container_event(action: &str) -> DockerEvent {
        DockerEvent {
            kind: ResourceKind::Container,
            action: action.into(),
            id: "web-id".into(),
        }
    }

    fn list_calls(fake: &FakeClient) -> usize {
        fake.calls()
            .iter()
            .filter(|c| *c == "list_containers:true")
            .count()
    }

    #[tokio::test(start_paused = true)]
    async fn container_event_triggers_reload() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = list_calls(&fake);
        fake.stop("web-id").await.unwrap();
        settle(&mut app, Message::Docker(container_event("die"))).await;
        assert_eq!(list_calls(&fake), before + 1);
        let web = app.containers.items.iter().find(|c| c.id == "web-id");
        assert!(matches!(
            web.map(|c| &c.state),
            Some(ContainerState::Exited { .. })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn burst_of_events_reloads_once() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = list_calls(&fake);
        let tasks: Vec<_> = (0..5)
            .map(|_| app.update(Message::Docker(container_event("die"))))
            .collect();
        tokio::time::advance(Duration::from_millis(100)).await;
        let mut produced = Vec::new();
        for task in tasks {
            produced.extend(run(task).await);
        }
        assert_eq!(produced.len(), 1, "only the first event schedules a reload");
        for m in produced {
            settle(&mut app, m).await;
        }
        assert_eq!(list_calls(&fake), before + 1);

        // The window is over: the next event schedules a new reload.
        settle(&mut app, Message::Docker(container_event("start"))).await;
        assert_eq!(list_calls(&fake), before + 2);
    }

    #[tokio::test(start_paused = true)]
    async fn reload_waits_for_the_debounce_window() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = list_calls(&fake);
        let task = app.update(Message::Docker(container_event("die")));
        let started = tokio::time::Instant::now();
        let produced = run(task).await;
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(matches!(produced[..], [Message::ReloadContainers]));
        // Nothing is listed until the debounce message is handled.
        assert_eq!(list_calls(&fake), before);
    }

    #[tokio::test(start_paused = true)]
    async fn volume_and_network_events_do_not_reload() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = list_calls(&fake);
        for kind in [ResourceKind::Volume, ResourceKind::Network] {
            let ev = DockerEvent {
                kind,
                action: "create".into(),
                id: "x".into(),
            };
            let handled = settle(&mut app, Message::Docker(ev.clone())).await;
            assert_eq!(handled.len(), 1);
        }
        assert_eq!(list_calls(&fake), before);
    }

    #[tokio::test(start_paused = true)]
    async fn reload_is_skipped_when_docker_is_unreachable() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Docker(container_event("die")));
        app.conn = Connection::Failed(DockerError::NotRunning);
        let before = list_calls(&fake);
        assert!(run(app.update(Message::ReloadContainers)).await.is_empty());
        assert_eq!(list_calls(&fake), before);
        // The flag was cleared: a later event can schedule again.
        app.conn = Connection::Ok;
        assert_eq!(
            run(app.update(Message::Docker(container_event("die"))))
                .await
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn events_ended_with_daemon_down_goes_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        fake.set_ping_error(Some(DockerError::NotRunning));
        let handled = settle(&mut app, Message::EventsEnded).await;
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(matches!(
            handled[..],
            [Message::EventsEnded, Message::Pinged(Err(_))]
        ));
        assert!(app.toasts.is_empty());
    }

    #[tokio::test]
    async fn events_ended_with_daemon_up_resubscribes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = app.events_generation;
        settle(&mut app, Message::EventsEnded).await;
        assert_eq!(app.conn, Connection::Ok);
        assert_eq!(app.events_generation, before + 1);
    }

    #[tokio::test]
    async fn every_successful_ping_renews_the_events_subscription() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        let first = EventsSource::new(&app);
        settle(&mut app, Message::Pinged(Ok(()))).await;
        let second = EventsSource::new(&app);
        assert_ne!(hash_of(&first), hash_of(&second));
        // Same generation, same identity: iced keeps the running subscription.
        assert_eq!(hash_of(&second), hash_of(&EventsSource::new(&app)));
    }

    fn hash_of(source: &EventsSource) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        h.finish()
    }

    #[test]
    fn events_subscription_runs_only_while_connected() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.conn = Connection::Ok;
        assert_eq!(app.subscription().units(), 1);
        app.conn = Connection::Checking;
        assert_eq!(app.subscription().units(), 0);
    }

    #[tokio::test]
    async fn event_messages_forward_events_and_end_when_the_stream_ends() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut stream = Box::pin(event_messages(fake.clone()));
        let ev = container_event("start");
        fake.push_event(ev.clone());
        assert!(matches!(
            stream.next().await,
            Some(Message::Docker(e)) if e == ev
        ));
        fake.set_ping_error(Some(DockerError::NotRunning));
        assert!(matches!(stream.next().await, Some(Message::EventsEnded)));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn event_messages_end_when_the_stream_cannot_open() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.set_ping_error(Some(DockerError::NotRunning));
        let mut stream = Box::pin(event_messages(fake.clone()));
        assert!(matches!(stream.next().await, Some(Message::EventsEnded)));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn action_events_refresh_the_list_without_extra_reload() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let mut stream = Box::pin(event_messages(fake.clone()));
        let before = list_calls(&fake);
        settle(
            &mut app,
            Message::Containers(containers::Msg::Stop("web-id".into())),
        )
        .await;
        assert_eq!(list_calls(&fake), before);
        // The events the action caused (die + stop) arrive as a burst.
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let Some(m) = stream.next().await else {
                panic!("the stream ended early");
            };
            tasks.push(app.update(m));
        }
        let mut produced = Vec::new();
        for task in tasks {
            produced.extend(run(task).await);
        }
        for m in produced {
            settle(&mut app, m).await;
        }
        assert_eq!(list_calls(&fake), before + 1);
        let web = app.containers.items.iter().find(|c| c.id == "web-id");
        assert!(matches!(
            web.map(|c| &c.state),
            Some(ContainerState::Exited { .. })
        ));
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
