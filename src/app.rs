use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use iced::widget::{Id, center, column, container, pane_grid, row, rule, stack, text};
use iced::{Element, Length, Subscription, Task, Theme};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::docker::DockerClient;
use crate::error::DockerError;
use crate::keymap;
use crate::model::{ContainerInfo, DockerEvent, ResourceKind};
use crate::ui::components::{ToastKind, modal};
use crate::ui::fonts::SANS_SEMIBOLD;
use crate::ui::prefs;
use crate::ui::style;
use crate::ui::theme::{ThemeMode, resolve};
use crate::ui::tokens::{BORDER_WIDTH, ROW_HEIGHT, SPACE_1, SPACE_2, SPACE_6, TEXT_LG};
use crate::views::containers::{self, ContainersState};
use crate::views::palette::{self, PaletteState};
use crate::views::widgets::{friendly_error, toast_stack};
use crate::views::{dock, onboarding, sidebar, status_bar};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Containers,
    Volumes,
    Networks,
}

/// Tabs of the bottom dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockTab {
    Logs,
    Stats,
    Terminal,
}

/// The bottom dock: which container it shows and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dock {
    pub container_id: String,
    pub tab: DockTab,
    /// The dock takes the whole main area.
    pub maximized: bool,
}

/// The two areas of the main column, divided by the draggable split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    List,
    Dock,
}

/// Share of the main column the list takes while the dock is open (list above, dock below).
const DOCK_SPLIT_RATIO: f32 = 0.6;

/// The list above and the dock below; the dock is only drawn while `App::dock` is set.
fn initial_panes() -> pane_grid::State<Pane> {
    pane_grid::State::with_configuration(pane_grid::Configuration::Split {
        axis: pane_grid::Axis::Horizontal,
        ratio: DOCK_SPLIT_RATIO,
        a: Box::new(pane_grid::Configuration::Pane(Pane::List)),
        b: Box::new(pane_grid::Configuration::Pane(Pane::Dock)),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Connection {
    Checking,
    Ok,
    Failed(DockerError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub kind: ToastKind,
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
    /// The wait after the events stream ended is over: check the daemon again.
    EventsRetry,
    /// The debounce window after a container event is over: reload the list.
    ReloadContainers,
    /// The system theme (light/dark) was read or changed.
    SystemTheme(iced::theme::Mode),
    ToggleSidebar,
    /// Shows a container in the dock, on the Logs tab.
    OpenDock(String),
    CloseDock,
    /// Closes the dock if it is open; otherwise opens it for the selected container.
    ToggleDock,
    DockTab(DockTab),
    ToggleMaximize,
    /// The user dragged the divider between the list and the dock.
    PaneResized(pane_grid::ResizeEvent),
    /// A button, shortcut or the palette asked for an action; the only way to run one.
    Action(ActionId),
    /// A key was pressed. `captured` is true when a widget (a text input) already used it.
    Key {
        key: iced::keyboard::Key,
        mods: iced::keyboard::Modifiers,
        captured: bool,
    },
    /// The palette's search text changed.
    PaletteQuery(String),
    /// Moves the highlighted palette result by this many rows.
    PaletteMove(i32),
    /// Runs the action and closes the palette; `None` (no result) does nothing.
    PaletteRun(Option<ActionId>),
    /// The results of the palette were scrolled to this offset.
    PaletteScrolled(f32),
}

pub struct App {
    client: Arc<dyn DockerClient>,
    pub conn: Connection,
    pub screen: Screen,
    pub containers: ContainersState,
    pub toasts: Vec<Toast>,
    pub theme_mode: ThemeMode,
    /// Last theme reported by the system; `Mode::None` until it answers or has no preference.
    pub system_mode: iced::theme::Mode,
    pub sidebar_collapsed: bool,
    pub dock: Option<Dock>,
    /// The command palette, while it is open.
    pub palette: Option<PaletteState>,
    /// Scroll offset of the palette results, to keep the cursor in view.
    palette_scroll_y: f32,
    /// Where preferences are saved; `None` keeps them in memory only.
    config_dir: Option<PathBuf>,
    /// Keeps the position of the divider between list and dock, also while the dock is closed.
    panes: pane_grid::State<Pane>,
    /// A ping is in flight; further `Retry` messages are ignored until it resolves.
    pinging: bool,
    /// Bumped on every successful ping so each (re)connection gets a fresh events subscription.
    events_generation: u64,
    /// A reload is waiting for the debounce window to end; further events join it.
    reload_scheduled: bool,
    /// Number of the latest list reload; answers from older ones are discarded.
    load_seq: u64,
    /// When the current events subscription was opened.
    events_started: Option<tokio::time::Instant>,
    /// How long to wait the next time the events stream ends.
    events_backoff: Duration,
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

/// First wait after the events stream ends; it doubles each time up to `EVENTS_BACKOFF_MAX`.
const EVENTS_BACKOFF_MIN: Duration = Duration::from_secs(1);
const EVENTS_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// A stream that stayed open this long counts as healthy and resets the backoff.
const EVENTS_STABLE: Duration = Duration::from_secs(30);

/// Most toasts on screen at once; the oldest go first.
const MAX_TOASTS: usize = 5;

impl App {
    /// Builds the app and starts the first connectivity check.
    pub fn new(client: Arc<dyn DockerClient>) -> (Self, Task<Message>) {
        Self::new_with_config(client, None)
    }

    /// Like [`App::new`], but loads the saved preferences from `config_dir` and saves later
    /// changes there.
    pub fn new_with_config(
        client: Arc<dyn DockerClient>,
        config_dir: Option<PathBuf>,
    ) -> (Self, Task<Message>) {
        let mut app = Self {
            client,
            conn: Connection::Checking,
            screen: Screen::Containers,
            containers: ContainersState::default(),
            toasts: Vec::new(),
            theme_mode: config_dir
                .as_deref()
                .map(prefs::load_theme_mode)
                .unwrap_or_default(),
            system_mode: iced::theme::Mode::None,
            sidebar_collapsed: false,
            dock: None,
            palette: None,
            palette_scroll_y: 0.0,
            config_dir,
            panes: initial_panes(),
            pinging: false,
            events_generation: 0,
            reload_scheduled: false,
            load_seq: 0,
            events_started: None,
            events_backoff: EVENTS_BACKOFF_MIN,
        };
        let task = Task::batch([
            app.start_ping(),
            iced::system::theme().map(Message::SystemTheme),
        ]);
        (app, task)
    }

    /// The iced theme for the chosen mode and the system preference.
    pub fn theme(&self) -> Theme {
        crate::ui::theme::theme(resolve(self.theme_mode, self.system_mode))
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
                // The row under the cursor is not drawn on the new screen, so no exit comes.
                self.containers.hovered = None;
                Task::none()
            }
            Message::Pinged(Ok(())) => {
                self.conn = Connection::Ok;
                self.events_generation += 1;
                self.events_started = Some(tokio::time::Instant::now());
                self.load_containers()
            }
            // Whatever the error, the daemon is not usable: the onboarding explains it and the
            // retry timer checks again. Carrying on would reopen the events stream in a loop.
            Message::Pinged(Err(e)) => {
                self.set_failed(e);
                Task::none()
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
            // takes over) or it is still there and the new ping renews the subscription. Wait
            // first, so a stream that fails right away does not turn this into a busy loop.
            Message::EventsEnded => self.on_events_ended(),
            Message::EventsRetry => self.start_ping(),
            Message::SystemTheme(mode) => {
                self.system_mode = mode;
                Task::none()
            }
            Message::ToggleSidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                Task::none()
            }
            Message::OpenDock(id) => {
                // Without Docker there is nothing to show; the onboarding is on screen.
                if self.conn != Connection::Ok {
                    return Task::none();
                }
                // Opening what is already shown keeps its tab and size.
                if self.dock.as_ref().is_none_or(|d| d.container_id != id) {
                    self.dock = Some(Dock {
                        container_id: id,
                        tab: DockTab::Logs,
                        maximized: false,
                    });
                }
                Task::none()
            }
            Message::CloseDock => {
                self.dock = None;
                Task::none()
            }
            Message::ToggleDock => {
                if self.conn != Connection::Ok {
                    return Task::none();
                }
                match (&self.dock, self.containers.selected.clone()) {
                    (None, Some(id)) => self.update(Message::OpenDock(id)),
                    _ => {
                        self.dock = None;
                        Task::none()
                    }
                }
            }
            Message::DockTab(tab) => {
                if let Some(dock) = &mut self.dock {
                    dock.tab = tab;
                }
                Task::none()
            }
            Message::ToggleMaximize => {
                if let Some(dock) = &mut self.dock {
                    dock.maximized = !dock.maximized;
                    if dock.maximized {
                        // The list is hidden, so the cursor exit of its row never arrives.
                        self.containers.hovered = None;
                    }
                }
                Task::none()
            }
            Message::PaneResized(event) => {
                self.panes.resize(event.split, event.ratio);
                Task::none()
            }
            Message::Action(id) => self.run_action(id),
            Message::Key {
                key,
                mods,
                captured,
            } => self.on_key(&key, mods, captured),
            Message::PaletteQuery(query) => self.palette_query(query),
            Message::PaletteMove(delta) => self.palette_move(delta),
            Message::PaletteRun(Some(id)) => {
                self.palette = None;
                self.run_action(id)
            }
            // Nothing matches: the palette stays, so the user can fix the query.
            Message::PaletteRun(None) => Task::none(),
            Message::PaletteScrolled(y) => {
                self.palette_scroll_y = y;
                Task::none()
            }
        }
    }

    /// Carries out an action, unless it is not available right now. Buttons, shortcuts and the
    /// palette all come through here.
    pub fn run_action(&mut self, id: ActionId) -> Task<Message> {
        use ActionId::*;
        use containers::Msg;
        if !actions::available(self, id) {
            return Task::none();
        }
        let selected = self.containers.selected.clone();
        // Only the actions on the selection read it; `available` made sure it exists then.
        let on_selected = |this: &mut Self, m: fn(String) -> Msg| match selected {
            Some(id) => this.update_containers(m(id)),
            None => Task::none(),
        };
        match id {
            GoContainers => self.update(Message::Navigate(Screen::Containers)),
            GoVolumes => self.update(Message::Navigate(Screen::Volumes)),
            GoNetworks => self.update(Message::Navigate(Screen::Networks)),
            ToggleSidebar => self.update(Message::ToggleSidebar),
            ToggleDock => self.update(Message::ToggleDock),
            FocusSearch => iced::widget::operation::focus(Id::new(containers::SEARCH_ID)),
            OpenPalette => {
                self.palette = Some(PaletteState::default());
                self.palette_scroll_y = 0.0;
                iced::widget::operation::focus(Id::new(palette::PALETTE_INPUT_ID))
            }
            SelectNext => self.select_step(true),
            SelectPrev => self.select_step(false),
            OpenSelected => on_selected(self, Msg::Open),
            StartStopSelected => {
                let running = self
                    .selected_container()
                    .is_some_and(|c| c.state.is_running());
                on_selected(self, if running { Msg::Stop } else { Msg::Start })
            }
            RestartSelected => on_selected(self, Msg::Restart),
            // Never removes right away: the dialog asks first.
            RemoveSelected => on_selected(self, Msg::AskRemove),
            ThemeSystem => self.set_theme_mode(ThemeMode::System),
            ThemeDark => self.set_theme_mode(ThemeMode::Dark),
            ThemeLight => self.set_theme_mode(ThemeMode::Light),
            Retry => self.update(Message::Retry),
            Escape => {
                self.close_top_layer();
                Task::none()
            }
        }
    }

    /// The selected container, if it is in the list.
    fn selected_container(&self) -> Option<&ContainerInfo> {
        let id = self.containers.selected.as_deref()?;
        self.containers.items.iter().find(|c| c.id == id)
    }

    /// Moves the selection one row down or up the visible list, stopping at its ends. Without a
    /// selection, down picks the first row and up the last. Scrolls the list if the new row is
    /// not fully on screen.
    fn select_step(&mut self, forward: bool) -> Task<Message> {
        let visible = self.containers.visible();
        let Some(last) = visible.len().checked_sub(1) else {
            return Task::none();
        };
        let current = self
            .containers
            .selected
            .as_deref()
            .and_then(|id| visible.iter().position(|c| c.id == id));
        let target = match (current, forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(i), true) => (i + 1).min(last),
            (Some(i), false) => i.saturating_sub(1),
        };
        let id = visible[target].id.clone();
        self.containers.selected = Some(id);
        let Some(y) = containers::scroll_offset(
            target,
            ROW_HEIGHT,
            self.containers.viewport_h,
            self.containers.scroll_y,
        ) else {
            return Task::none();
        };
        // Programmatic scrolling is not reported back, so remember where it went.
        self.containers.scroll_y = y;
        iced::widget::operation::scroll_to(
            Id::new(containers::LIST_ID),
            iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    }

    /// Esc closes one layer, the topmost: the command palette, then the removal dialog, then the
    /// dock, then the search.
    fn close_top_layer(&mut self) {
        if self.palette.take().is_some() {
            return;
        }
        if self.containers.confirm_remove.take().is_some() {
            return;
        }
        if self.dock.is_some() {
            let _ = self.update(Message::CloseDock);
            return;
        }
        if !self.containers.query.is_empty() {
            self.containers.query.clear();
            self.containers.keep_selection_visible();
        }
    }

    /// Applies the theme and saves the choice. The file is tiny, so it is written right here:
    /// two quick changes cannot race and the last one wins. A failure is only logged.
    fn set_theme_mode(&mut self, mode: ThemeMode) -> Task<Message> {
        self.theme_mode = mode;
        if let Some(dir) = &self.config_dir
            && let Err(e) = prefs::save_theme_mode(dir, mode)
        {
            tracing::warn!("could not save the theme: {e}");
        }
        Task::none()
    }

    /// New search text: the highlight goes back to the best match, at the top of the results.
    fn palette_query(&mut self, query: String) -> Task<Message> {
        let Some(state) = &mut self.palette else {
            return Task::none();
        };
        state.query = query;
        state.cursor = 0;
        if self.palette_scroll_y == 0.0 {
            return Task::none();
        }
        self.palette_scroll_y = 0.0;
        self.scroll_palette_to(0.0)
    }

    /// Moves the highlight, stopping at the first and last result, and scrolls it into view.
    fn palette_move(&mut self, delta: i32) -> Task<Message> {
        let Some(state) = &self.palette else {
            return Task::none();
        };
        let count = palette::matches(self, &state.query).len();
        let last = count.saturating_sub(1) as i64;
        let cursor = (state.cursor as i64 + i64::from(delta)).clamp(0, last) as usize;
        if let Some(state) = &mut self.palette {
            state.cursor = cursor;
        }
        let Some(y) = containers::scroll_offset(
            cursor,
            ROW_HEIGHT,
            palette::list_height(count),
            self.palette_scroll_y,
        ) else {
            return Task::none();
        };
        // Programmatic scrolling is not reported back, so remember where it went.
        self.palette_scroll_y = y;
        self.scroll_palette_to(y)
    }

    fn scroll_palette_to(&self, y: f32) -> Task<Message> {
        iced::widget::operation::scroll_to(
            Id::new(palette::PALETTE_LIST_ID),
            iced::widget::operation::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )
    }

    /// A key press: turns it into an action, if it stands for one.
    fn on_key(
        &mut self,
        key: &iced::keyboard::Key,
        mods: iced::keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        // The palette sits above everything: it takes the arrows and Enter, even when its text
        // input already used them, and leaves only Esc and Ctrl shortcuts to the keymap.
        if let Some(state) = &self.palette {
            use iced::keyboard::Key;
            use iced::keyboard::key::Named;
            if !mods.control() {
                match key {
                    Key::Named(Named::ArrowDown) => return self.palette_move(1),
                    Key::Named(Named::ArrowUp) => return self.palette_move(-1),
                    Key::Named(Named::Enter) => {
                        let chosen = palette::matches(self, &state.query)
                            .get(state.cursor)
                            .copied();
                        return self.update(Message::PaletteRun(chosen));
                    }
                    _ => {}
                }
            }
            if !mods.control() && *key != Key::Named(Named::Escape) {
                return Task::none();
            }
        }
        // With the removal dialog open, Enter presses its main button, even if a text input
        // still holds the event. Esc is left to the keymap and cancels.
        let dialog_open = containers::remove_force(&self.containers);
        if let Some(force) = dialog_open
            && *key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
            && mods.is_empty()
        {
            return self.update_containers(containers::Msg::ConfirmRemove { force });
        }
        let Some(id) = keymap::resolve(key, mods, captured) else {
            return Task::none();
        };
        // The dialog only answers to Enter and Esc; nothing else reaches the window behind it.
        if dialog_open.is_some() && id != ActionId::Escape {
            return Task::none();
        }
        // A shortcut that works on the window behind the palette closes it first.
        if self.palette.is_some()
            && !matches!(id, ActionId::Escape | ActionId::OpenPalette)
            && actions::available(self, id)
        {
            self.palette = None;
        }
        self.run_action(id)
    }

    fn update_containers(&mut self, m: containers::Msg) -> Task<Message> {
        use containers::Msg;
        match m {
            // Reloads can overlap: only the latest one counts.
            Msg::Loaded(seq, _) if seq != self.load_seq => Task::none(),
            Msg::Loaded(_, Ok(items)) => {
                self.containers.set_items(items);
                // The container is gone (removed here or elsewhere): close the dock quietly.
                if let Some(dock) = &self.dock
                    && !self
                        .containers
                        .items
                        .iter()
                        .any(|c| c.id == dock.container_id)
                {
                    self.dock = None;
                }
                // The container to remove is gone: nothing left to confirm.
                if let Some(id) = &self.containers.confirm_remove
                    && !self.containers.items.iter().any(|c| &c.id == id)
                {
                    self.containers.confirm_remove = None;
                }
                Task::none()
            }
            Msg::Loaded(_, Err(e)) => self.fail(e),
            Msg::Query(query) => {
                self.containers.query = query;
                self.containers.keep_selection_visible();
                Task::none()
            }
            Msg::ToggleRunning => {
                self.containers.show_running = !self.containers.show_running;
                self.containers.keep_selection_visible();
                Task::none()
            }
            Msg::ToggleStopped => {
                self.containers.show_stopped = !self.containers.show_stopped;
                self.containers.keep_selection_visible();
                Task::none()
            }
            Msg::Select(id) => {
                self.containers.selected = Some(id);
                Task::none()
            }
            Msg::Hover(id) => {
                self.containers.hovered = Some(id);
                Task::none()
            }
            Msg::Leave(id) => {
                if self.containers.hovered.as_deref() == Some(id.as_str()) {
                    self.containers.hovered = None;
                }
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
            Msg::Open(id) => {
                self.containers.selected = Some(id.clone());
                self.update(Message::OpenDock(id))
            }
            Msg::Scrolled { offset, height } => {
                self.containers.scroll_y = offset;
                self.containers.viewport_h = height;
                Task::none()
            }
            Msg::Resized(height) => {
                self.containers.viewport_h = height;
                Task::none()
            }
        }
    }

    /// Container events refresh the list, debounced so a burst causes a single reload.
    /// Volume and network events are handled in phase 4.
    fn on_docker_event(&mut self, ev: DockerEvent) -> Task<Message> {
        // The stream is alive.
        self.events_backoff = EVENTS_BACKOFF_MIN;
        if ev.kind != ResourceKind::Container || self.reload_scheduled {
            return Task::none();
        }
        self.reload_scheduled = true;
        Task::perform(tokio::time::sleep(RELOAD_DEBOUNCE), |()| {
            Message::ReloadContainers
        })
    }

    /// Waits (with backoff) before checking the daemon again after the events stream ended.
    fn on_events_ended(&mut self) -> Task<Message> {
        let lived = self
            .events_started
            .map_or(Duration::ZERO, |started| started.elapsed());
        if lived >= EVENTS_STABLE {
            self.events_backoff = EVENTS_BACKOFF_MIN;
        }
        let wait = self.events_backoff;
        self.events_backoff = (wait * 2).min(EVENTS_BACKOFF_MAX);
        Task::perform(tokio::time::sleep(wait), |()| Message::EventsRetry)
    }

    /// Docker is unusable: shows the onboarding and drops anything tied to the old connection.
    fn set_failed(&mut self, e: DockerError) {
        self.conn = Connection::Failed(e);
        self.containers.confirm_remove = None;
        self.containers.hovered = None;
        self.dock = None;
    }

    /// Connection problems send the user to the onboarding; anything else becomes a toast.
    fn fail(&mut self, e: DockerError) -> Task<Message> {
        if e.is_connection() {
            self.set_failed(e);
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

    fn load_containers(&mut self) -> Task<Message> {
        self.load_seq += 1;
        let seq = self.load_seq;
        Task::perform(list_all(self.client.clone()), move |r| {
            Message::Containers(containers::Msg::Loaded(seq, r))
        })
    }

    /// Shows an error toast, unless one with the same text is already visible. At most
    /// `MAX_TOASTS` stay on screen; the oldest are dropped.
    pub fn push_toast(&mut self, text: String, details: Option<String>) {
        if self.toasts.iter().any(|t| t.text == text) {
            return;
        }
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast {
            kind: ToastKind::Error,
            text,
            details,
            expanded: false,
        });
    }

    pub fn view(&self) -> Element<'_, Message> {
        let main: Element<'_, Message> = match &self.conn {
            Connection::Checking => center(text(t!("connection.checking").into_owned())).into(),
            Connection::Failed(e) => onboarding::view(e),
            Connection::Ok => self.workspace(),
        };
        let count = (self.conn == Connection::Ok).then_some(self.containers.items.len());
        let body = column![
            row![
                sidebar::view(&self.screen, self.sidebar_collapsed, count),
                container(main).width(Length::Fill).height(Length::Fill),
            ]
            .height(Length::Fill),
            rule::horizontal(BORDER_WIDTH).style(style::rule_border),
            status_bar::view(&self.conn, &self.containers.items),
        ];
        let body: Element<'_, Message> = container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::container::bg)
            .into();
        // The dialog dims the whole window, sidebar and status bar included.
        let body = match containers::remove_dialog(&self.containers) {
            Some(card) => modal(
                body,
                card,
                Message::Containers(containers::Msg::CancelRemove),
            ),
            None => body,
        };
        let body = match &self.palette {
            Some(state) => stack![body, palette::view(self, state)].into(),
            None => body,
        };
        if self.toasts.is_empty() {
            body
        } else {
            stack![body, toast_stack(&self.toasts)].into()
        }
    }

    /// The screen, with the dock below it while one is open: alone when maximized, otherwise
    /// side by side with the screen in the pane grid.
    fn workspace(&self) -> Element<'_, Message> {
        let Some(open) = &self.dock else {
            return self.screen_content();
        };
        let dock_view = || dock::view(open, self.container_name(&open.container_id));
        if open.maximized {
            return dock_view();
        }
        pane_grid::PaneGrid::new(&self.panes, |_, pane, _| {
            pane_grid::Content::new(match pane {
                Pane::List => self.screen_content(),
                Pane::Dock => dock_view(),
            })
        })
        .spacing(BORDER_WIDTH)
        .on_resize(SPACE_1, Message::PaneResized)
        .style(style::pane_grid_style)
        .into()
    }

    /// Name of a container, if it is in the list.
    fn container_name(&self, id: &str) -> Option<&str> {
        self.containers
            .items
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }

    fn screen_content(&self) -> Element<'_, Message> {
        let title = match &self.screen {
            Screen::Containers => return containers::view(&self.containers),
            Screen::Volumes => t!("sidebar.volumes"),
            Screen::Networks => t!("sidebar.networks"),
        };
        container(
            column![
                text(title.into_owned()).size(TEXT_LG).font(SANS_SEMIBOLD),
                text(t!("common.coming_soon").into_owned()).style(style::text::muted),
            ]
            .spacing(SPACE_2),
        )
        .padding(SPACE_6)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// While Docker is reachable, listen to its events; while it is not, ping again every few
    /// seconds.
    pub fn subscription(&self) -> Subscription<Message> {
        let system_theme = iced::system::theme_changes().map(Message::SystemTheme);
        let conn = match self.conn {
            Connection::Ok => Subscription::run_with(EventsSource::new(self), |source| {
                event_messages(source.client.clone())
            }),
            Connection::Failed(_) => iced::time::every(RETRY_INTERVAL).map(|_| Message::Retry),
            Connection::Checking => Subscription::none(),
        };
        // Not a closure: iced needs a plain `fn` to identify the subscription.
        let keys = iced::event::listen_with(key_event);
        Subscription::batch([system_theme, conn, keys])
    }
}

/// A key press as a message; any other event is dropped.
fn key_event(
    event: iced::event::Event,
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::event::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key, modifiers, ..
        }) => Some(Message::Key {
            key,
            mods: modifiers,
            captured: status == iced::event::Status::Captured,
        }),
        _ => None,
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
        let _ = app.update(Message::Navigate(Screen::Networks));
        assert_eq!(app.screen, Screen::Networks);
    }

    #[test]
    fn system_theme_change_updates_app_theme() {
        use crate::ui::theme::palette;
        use crate::ui::tokens::{DARK, LIGHT};
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        // No system preference yet: dark.
        assert_eq!(palette(&app.theme()), &DARK);
        let _ = app.update(Message::SystemTheme(iced::theme::Mode::Light));
        assert_eq!(palette(&app.theme()), &LIGHT);
        let _ = app.update(Message::SystemTheme(iced::theme::Mode::None));
        assert_eq!(palette(&app.theme()), &DARK);
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
    async fn non_connection_ping_error_goes_to_onboarding_without_loading() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        let before = app.events_generation;
        let e = DockerError::Other("boom".into());
        let handled = settle(&mut app, Message::Pinged(Err(e.clone()))).await;
        assert_eq!(app.conn, Connection::Failed(e));
        assert_eq!(handled.len(), 1, "nothing follows the failed ping");
        assert!(app.toasts.is_empty());
        assert_eq!(app.events_generation, before, "no events subscription");
        assert!(fake.calls().is_empty(), "{:?}", fake.calls());
        // It is retried every few seconds like any other connection failure.
        assert_eq!(app.subscription().units(), 3);
    }

    #[tokio::test]
    async fn connection_lost_while_loading_returns_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        let _ = app.update(Message::Pinged(Ok(())));
        let seq = app.load_seq;
        let _ = app.update(Message::Containers(containers::Msg::Loaded(
            seq,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
    }

    #[test]
    fn retry_tick_runs_only_while_failed() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.conn = Connection::Checking;
        assert_eq!(app.subscription().units(), 2);
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert_eq!(app.subscription().units(), 3);
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
        assert_eq!(app.toasts[0].kind, ToastKind::Error);
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
        assert!(app.containers.show_running && app.containers.show_stopped);
        let _ = app.update(Message::Containers(containers::Msg::ToggleStopped));
        assert!(!app.containers.show_stopped);
        let _ = app.update(Message::Containers(containers::Msg::ToggleRunning));
        assert!(!app.containers.show_running);
        let _ = app.update(Message::Containers(containers::Msg::ToggleStopped));
        assert!(app.containers.show_stopped);
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

    #[tokio::test(start_paused = true)]
    async fn events_ended_with_daemon_down_goes_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        fake.set_ping_error(Some(DockerError::NotRunning));
        let handled = settle(&mut app, Message::EventsEnded).await;
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(matches!(
            handled[..],
            [
                Message::EventsEnded,
                Message::EventsRetry,
                Message::Pinged(Err(_))
            ]
        ));
        assert!(app.toasts.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn events_ended_with_daemon_up_resubscribes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = app.events_generation;
        settle(&mut app, Message::EventsEnded).await;
        assert_eq!(app.conn, Connection::Ok);
        assert_eq!(app.events_generation, before + 1);
    }

    /// The events stream ends, as the real subscription would report, and the app reacts.
    /// Returns how long the app waited before pinging again.
    async fn end_events(app: &mut App) -> Duration {
        let started = tokio::time::Instant::now();
        let handled = settle(app, Message::EventsEnded).await;
        assert!(
            matches!(handled[1], Message::EventsRetry),
            "the ping must wait: {handled:?}"
        );
        started.elapsed()
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn assert_about(got: Duration, want: Duration) {
        assert!(
            got >= want && got < want + Duration::from_millis(100),
            "waited {got:?}, expected {want:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn events_that_fail_at_once_do_not_make_the_app_spin() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        fake.set_events_error(Some(DockerError::Other("boom".into())));
        let ping_count = |fake: &FakeClient| fake.calls().iter().filter(|c| *c == "ping").count();
        let before = ping_count(&fake);
        let started = tokio::time::Instant::now();
        let mut rounds = 0;
        while started.elapsed() < secs(60) && rounds < 200 {
            // What the subscription does when `events()` fails right away.
            let first = Box::pin(event_messages(fake.clone())).next().await;
            assert!(matches!(first, Some(Message::EventsEnded)));
            settle(&mut app, Message::EventsEnded).await;
            rounds += 1;
        }
        let pings = ping_count(&fake) - before;
        assert!(pings <= 8, "{pings} pings in a minute");
        assert_eq!(app.conn, Connection::Ok);
        assert!(app.toasts.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn events_backoff_doubles_up_to_thirty_seconds() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for want in [1, 2, 4, 8, 16, 30, 30] {
            assert_about(end_events(&mut app).await, secs(want));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn events_backoff_restarts_after_an_event() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for want in [1, 2, 4] {
            assert_about(end_events(&mut app).await, secs(want));
        }
        let volume_event = DockerEvent {
            kind: ResourceKind::Volume,
            action: "create".into(),
            id: "v".into(),
        };
        let _ = app.update(Message::Docker(volume_event));
        assert_about(end_events(&mut app).await, secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn events_backoff_restarts_after_a_stable_stream() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for want in [1, 2, 4] {
            assert_about(end_events(&mut app).await, secs(want));
        }
        tokio::time::advance(secs(60)).await;
        assert_about(end_events(&mut app).await, secs(1));
        assert_about(end_events(&mut app).await, secs(2));
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
        // The system theme and keyboard subscriptions are always there; the events one only while
        // connected.
        assert_eq!(app.subscription().units(), 3);
        app.conn = Connection::Checking;
        assert_eq!(app.subscription().units(), 2);
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

    #[tokio::test]
    async fn an_older_load_cannot_overwrite_a_newer_one() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let older = run(app.update(Message::ReloadContainers)).await;
        fake.stop("web-id").await.unwrap();
        let newer = run(app.update(Message::ReloadContainers)).await;
        assert_eq!((older.len(), newer.len()), (1, 1));
        // The newer answer arrives first, the older one afterwards.
        for m in newer.into_iter().chain(older) {
            let _ = app.update(m);
        }
        let web = app.containers.items.iter().find(|c| c.id == "web-id");
        assert!(matches!(
            web.map(|c| &c.state),
            Some(ContainerState::Exited { .. })
        ));
    }

    #[tokio::test]
    async fn an_older_load_error_is_ignored_too() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let stale = app.load_seq;
        let _ = app.update(Message::ReloadContainers);
        let _ = app.update(Message::Containers(containers::Msg::Loaded(
            stale,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.conn, Connection::Ok);
    }

    #[test]
    fn toasts_are_capped_dropping_the_oldest() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        for i in 0..8 {
            app.push_toast(format!("toast {i}"), None);
        }
        let texts: Vec<_> = app.toasts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            texts,
            ["toast 3", "toast 4", "toast 5", "toast 6", "toast 7"]
        );
    }

    #[test]
    fn a_toast_with_the_same_text_is_not_duplicated() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.push_toast("same".into(), Some("first".into()));
        app.push_toast("other".into(), None);
        app.push_toast("same".into(), Some("second".into()));
        assert_eq!(app.toasts.len(), 2);
        // Once dismissed, it can show up again.
        let _ = app.update(Message::DismissToast(0));
        app.push_toast("same".into(), None);
        assert_eq!(app.toasts.len(), 2);
    }

    #[tokio::test]
    async fn losing_the_connection_closes_the_remove_dialog() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "web-id".into(),
        )));
        fake.set_ping_error(Some(DockerError::NotRunning));
        settle(&mut app, Message::Retry).await;
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert!(app.containers.confirm_remove.is_none());

        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "web-id".into(),
        )));
        app.conn = Connection::Ok;
        let seq = app.load_seq;
        let _ = app.update(Message::Containers(containers::Msg::Loaded(
            seq,
            Err(DockerError::NotRunning),
        )));
        assert!(app.containers.confirm_remove.is_none());
    }

    fn web_dock() -> Dock {
        Dock {
            container_id: "web-id".into(),
            tab: DockTab::Logs,
            maximized: false,
        }
    }

    #[test]
    fn open_dock_from_containers_msg() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.conn = Connection::Ok;
        assert_eq!(app.dock, None);
        let _ = app.update(Message::Containers(containers::Msg::Open("web-id".into())));
        assert_eq!(app.dock, Some(web_dock()));
        // Opening another container replaces the dock and starts on Logs, not maximized.
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.update(Message::ToggleMaximize);
        let _ = app.update(Message::OpenDock("db-id".into()));
        assert_eq!(
            app.dock,
            Some(Dock {
                container_id: "db-id".into(),
                ..web_dock()
            })
        );
    }

    #[test]
    fn reopening_the_same_container_keeps_the_dock_as_it_is() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.conn = Connection::Ok;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(app.dock.map(|d| d.tab), Some(DockTab::Stats));
    }

    #[test]
    fn close_and_toggle_dock() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.conn = Connection::Ok;
        let _ = app.update(Message::OpenDock("web-id".into()));

        let _ = app.update(Message::DockTab(DockTab::Stats));
        assert_eq!(app.dock.as_ref().map(|d| d.tab), Some(DockTab::Stats));
        let _ = app.update(Message::ToggleMaximize);
        assert_eq!(app.dock.as_ref().map(|d| d.maximized), Some(true));
        let _ = app.update(Message::ToggleMaximize);
        assert_eq!(app.dock.as_ref().map(|d| d.maximized), Some(false));

        let _ = app.update(Message::ToggleDock);
        assert_eq!(app.dock, None);
        // Without a dock and without a selection there is nothing to open.
        let _ = app.update(Message::ToggleDock);
        assert_eq!(app.dock, None);

        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::CloseDock);
        assert_eq!(app.dock, None);
        // Nothing to act on: no panic, no dock.
        let _ = app.update(Message::CloseDock);
        let _ = app.update(Message::ToggleMaximize);
        let _ = app.update(Message::DockTab(DockTab::Terminal));
        assert_eq!(app.dock, None);
    }

    #[test]
    fn select_and_double_click_open() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        app.conn = Connection::Ok;
        let _ = app.update(Message::Containers(containers::Msg::Select(
            "web-id".into(),
        )));
        assert_eq!(app.containers.selected.as_deref(), Some("web-id"));
        assert_eq!(app.dock, None, "a single click only selects");
        let _ = app.update(Message::Containers(containers::Msg::Open("web-id".into())));
        assert_eq!(app.dock, Some(web_dock()));
        assert_eq!(app.containers.selected.as_deref(), Some("web-id"));
    }

    #[test]
    fn hover_follows_the_rows_under_the_cursor() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        let hover = |id: &str| Message::Containers(containers::Msg::Hover(id.into()));
        let leave = |id: &str| Message::Containers(containers::Msg::Leave(id.into()));
        let _ = app.update(hover("a"));
        assert_eq!(app.containers.hovered.as_deref(), Some("a"));
        // Moving up: the new row reports before the old one leaves.
        let _ = app.update(hover("b"));
        let _ = app.update(leave("a"));
        assert_eq!(app.containers.hovered.as_deref(), Some("b"));
        let _ = app.update(leave("b"));
        assert_eq!(app.containers.hovered, None);
    }

    #[tokio::test]
    async fn selection_moves_to_neighbor_when_removed() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let order: Vec<String> = app
            .containers
            .visible()
            .iter()
            .map(|c| c.id.clone())
            .collect();
        assert!(order.len() >= 3);
        let _ = app.update(Message::Containers(containers::Msg::Select(
            order[1].clone(),
        )));
        fake.remove(&order[1], true).await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert_eq!(app.containers.selected.as_ref(), Some(&order[2]));

        // The last one goes: the new last one is selected.
        let last = order.last().unwrap().clone();
        let _ = app.update(Message::Containers(containers::Msg::Select(last.clone())));
        fake.remove(&last, true).await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        let now: Vec<String> = app
            .containers
            .visible()
            .iter()
            .map(|c| c.id.clone())
            .collect();
        assert_eq!(app.containers.selected.as_ref(), now.last());
    }

    #[tokio::test]
    async fn selection_survives_reconnecting_only_if_the_container_exists() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::Select(
            "web-id".into(),
        )));
        let _ = app.update(Message::Pinged(Err(DockerError::NotRunning)));
        settle(&mut app, Message::Pinged(Ok(()))).await;
        assert_eq!(app.containers.selected.as_deref(), Some("web-id"));

        let _ = app.update(Message::Pinged(Err(DockerError::NotRunning)));
        fake.remove("web-id", true).await.unwrap();
        settle(&mut app, Message::Pinged(Ok(()))).await;
        assert_ne!(app.containers.selected.as_deref(), Some("web-id"));
        assert!(
            app.containers.selected.as_ref().is_none_or(|id| app
                .containers
                .items
                .iter()
                .any(|c| &c.id == id))
        );
    }

    #[tokio::test]
    async fn toggle_dock_opens_selected() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::Select(
            "web-id".into(),
        )));
        let _ = app.update(Message::ToggleDock);
        assert_eq!(app.dock, Some(web_dock()));
        let _ = app.update(Message::ToggleDock);
        assert_eq!(app.dock, None);
    }

    #[tokio::test]
    async fn remove_dialog_covers_the_whole_window() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.containers.confirm_remove = Some("db-id".into());
        // Dock open, dialog open, sidebar collapsed: all layouts build.
        let _ = app.view();
        app.sidebar_collapsed = true;
        let _ = app.view();
    }

    #[tokio::test]
    async fn dock_stays_open_while_its_container_exists() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        settle(&mut app, Message::ReloadContainers).await;
        assert_eq!(app.dock, Some(web_dock()));
    }

    #[tokio::test]
    async fn dock_closes_when_container_disappears() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::ToggleMaximize);
        fake.remove("web-id", true).await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert!(!app.containers.items.iter().any(|c| c.id == "web-id"));
        assert_eq!(app.dock, None);
        assert!(app.toasts.is_empty());
        assert_eq!(app.conn, Connection::Ok);
        let _ = app.view();
    }

    #[tokio::test]
    async fn dock_closes_when_the_app_removes_its_container() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("worker-id".into()));
        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "worker-id".into(),
        )));
        settle(
            &mut app,
            Message::Containers(containers::Msg::ConfirmRemove { force: false }),
        )
        .await;
        // The `destroy` event the daemon sends leads to the reload.
        settle(&mut app, Message::ReloadContainers).await;
        assert_eq!(app.dock, None);
        assert!(app.toasts.is_empty());
    }

    #[tokio::test]
    async fn connection_loss_closes_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::Pinged(Err(DockerError::NotRunning)));
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert_eq!(app.dock, None);

        // The same through a failed reload, and after coming back the dock stays closed.
        app.conn = Connection::Ok;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let seq = app.load_seq;
        let _ = app.update(Message::Containers(containers::Msg::Loaded(
            seq,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.dock, None);
        settle(&mut app, Message::Pinged(Ok(()))).await;
        assert_eq!(app.conn, Connection::Ok);
        assert_eq!(app.dock, None);
    }

    #[tokio::test]
    async fn dock_does_not_open_while_docker_is_down() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::Select(
            "web-id".into(),
        )));
        let _ = app.update(Message::Pinged(Err(DockerError::NotRunning)));
        let _ = app.update(Message::ToggleDock);
        assert_eq!(app.dock, None);
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(app.dock, None);
        let _ = app.update(Message::Containers(containers::Msg::Open("web-id".into())));
        assert_eq!(app.dock, None);
        // Back online the dock is still closed.
        settle(&mut app, Message::Pinged(Ok(()))).await;
        assert_eq!(app.conn, Connection::Ok);
        assert_eq!(app.dock, None);
    }

    #[tokio::test]
    async fn selection_moves_to_the_first_visible_row_when_a_filter_hides_it() {
        use containers::Msg;
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let first_visible = |app: &App| app.containers.visible().first().map(|c| c.id.clone());
        let select = |app: &mut App, id: &str| {
            let _ = app.update(Message::Containers(Msg::Select(id.into())));
        };

        // The stopped chip is switched off while a stopped container is selected.
        select(&mut app, "worker-id");
        let _ = app.update(Message::Containers(Msg::ToggleStopped));
        assert_eq!(app.containers.selected, first_visible(&app));
        assert!(app.containers.selected.is_some());
        // Nothing visible at all: no selection.
        let _ = app.update(Message::Containers(Msg::ToggleRunning));
        assert_eq!(app.containers.selected, None);

        // A selection that stays visible is kept.
        let _ = app.update(Message::Containers(Msg::ToggleRunning));
        select(&mut app, "web-id");
        let _ = app.update(Message::Containers(Msg::Query("web".into())));
        assert_eq!(app.containers.selected.as_deref(), Some("web-id"));

        // The search hides the selection.
        select(&mut app, "web-id");
        let _ = app.update(Message::Containers(Msg::Query("db".into())));
        assert_eq!(app.containers.selected.as_deref(), Some("db-id"));
        let _ = app.update(Message::Containers(Msg::Query("zzz".into())));
        assert_eq!(app.containers.selected, None);

        // Switching the running chip off hides a running selection.
        let _ = app.update(Message::Containers(Msg::Query(String::new())));
        let _ = app.update(Message::Containers(Msg::ToggleStopped));
        select(&mut app, "web-id");
        let _ = app.update(Message::Containers(Msg::ToggleRunning));
        assert_eq!(app.containers.selected, first_visible(&app));
        assert_eq!(app.containers.selected.as_deref(), Some("worker-id"));
    }

    #[tokio::test]
    async fn reload_moves_a_selection_that_a_filter_now_hides() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::ToggleStopped));
        let _ = app.update(Message::Containers(containers::Msg::Select(
            "web-id".into(),
        )));
        // `web` stops while the Stopped chip is off: it still exists, but is hidden.
        fake.stop("web-id").await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert!(app.containers.items.iter().any(|c| c.id == "web-id"));
        assert_eq!(app.containers.selected.as_deref(), Some("db-id"));
    }

    #[tokio::test]
    async fn hover_is_cleared_on_navigation_failure_and_maximize() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let hover = |app: &mut App| {
            let _ = app.update(Message::Containers(containers::Msg::Hover("web-id".into())));
            assert!(app.containers.hovered.is_some());
        };

        hover(&mut app);
        let _ = app.update(Message::Navigate(Screen::Volumes));
        assert_eq!(app.containers.hovered, None);

        hover(&mut app);
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::ToggleMaximize);
        assert_eq!(app.containers.hovered, None);
        // Restoring does not need to touch it.
        hover(&mut app);
        let _ = app.update(Message::ToggleMaximize);
        assert!(app.containers.hovered.is_some());

        let _ = app.update(Message::Pinged(Err(DockerError::NotRunning)));
        assert_eq!(app.containers.hovered, None);
    }

    #[tokio::test]
    async fn remove_dialog_closes_quietly_when_its_container_is_gone() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "web-id".into(),
        )));
        fake.remove("web-id", true).await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert!(app.containers.confirm_remove.is_none());
        assert!(app.toasts.is_empty());

        // A dialog for a container that is still there stays open across reloads.
        let _ = app.update(Message::Containers(containers::Msg::AskRemove(
            "db-id".into(),
        )));
        settle(&mut app, Message::ReloadContainers).await;
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("db-id"));
    }

    #[test]
    fn toggle_sidebar() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        let before = app.sidebar_collapsed;
        let _ = app.update(Message::ToggleSidebar);
        assert_ne!(app.sidebar_collapsed, before);
        let _ = app.update(Message::ToggleSidebar);
        assert_eq!(app.sidebar_collapsed, before);
    }

    /// Ratio of the only split of the pane grid.
    fn split_of(app: &App) -> (pane_grid::Split, f32) {
        match app.panes.layout() {
            pane_grid::Node::Split { id, ratio, .. } => (*id, *ratio),
            pane_grid::Node::Pane(_) => panic!("the panes are not split"),
        }
    }

    #[test]
    fn dock_starts_at_sixty_percent_and_resizing_is_kept() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        let (split, ratio) = split_of(&app);
        assert_eq!(ratio, DOCK_SPLIT_RATIO);
        assert_eq!(ratio, 0.6);
        let _ = app.update(Message::PaneResized(pane_grid::ResizeEvent {
            split,
            ratio: 0.3,
        }));
        assert_eq!(split_of(&app).1, 0.3);
        // Closing and reopening the dock keeps the divider where the user left it.
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::CloseDock);
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(split_of(&app).1, 0.3);
    }

    #[test]
    fn view_builds_for_every_layout() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = app_with(&fake);
        app.containers.items = futures::executor::block_on(fake.list_containers(true)).unwrap();
        app.push_toast("a".into(), Some("detail".into()));
        for conn in [
            Connection::Checking,
            Connection::Ok,
            Connection::Failed(DockerError::NotRunning),
        ] {
            app.conn = conn;
            for collapsed in [false, true] {
                app.sidebar_collapsed = collapsed;
                for dock in [
                    None,
                    Some(web_dock()),
                    Some(Dock {
                        maximized: true,
                        ..web_dock()
                    }),
                    // A dock whose container is unknown must not panic either.
                    Some(Dock {
                        container_id: "gone-id".into(),
                        tab: DockTab::Terminal,
                        maximized: false,
                    }),
                ] {
                    app.dock = dock;
                    for screen in [Screen::Containers, Screen::Volumes, Screen::Networks] {
                        app.screen = screen;
                        let _ = app.view();
                    }
                }
            }
        }
    }

    #[test]
    fn dock_and_status_texts_are_translated() {
        for key in [
            "status.connected",
            "status.checking",
            "status.counts",
            "status.toggle_sidebar",
            "status.toggle_dock",
            "status.palette",
            "dock.empty_tab",
            "dock.tab.logs",
            "dock.tab.stats",
            "dock.tab.terminal",
            "dock.maximize",
            "dock.restore",
            "dock.close",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(key, locale = locale, running = 1, stopped = 2),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
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
            for screen in [Screen::Containers, Screen::Volumes, Screen::Networks] {
                app.screen = screen;
                let _ = app.view();
            }
        }
        let _ = app.subscription();
    }

    // ---- Actions and keyboard (task 2.6.1) ----

    fn key_msg(c: &str, captured: bool) -> Message {
        Message::Key {
            key: iced::keyboard::Key::Character(c.into()),
            mods: iced::keyboard::Modifiers::empty(),
            captured,
        }
    }

    /// Visible order of the sample data: running first by name, so `db`, `web`, `worker`.
    fn selected_id(app: &App) -> Option<&str> {
        app.containers.selected.as_deref()
    }

    #[tokio::test]
    async fn select_next_prev_bounds() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = None;
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(selected_id(&app), Some("db-id"), "no selection: first");
        let _ = app.run_action(ActionId::SelectNext);
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(selected_id(&app), Some("worker-id"));
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(selected_id(&app), Some("worker-id"), "does not wrap around");

        let _ = app.run_action(ActionId::SelectPrev);
        let _ = app.run_action(ActionId::SelectPrev);
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(selected_id(&app), Some("db-id"), "does not wrap around");

        app.containers.selected = None;
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(selected_id(&app), Some("worker-id"), "no selection: last");
    }

    #[tokio::test]
    async fn select_follows_the_filtered_list() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.show_stopped = false;
        app.containers.selected = None;
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(selected_id(&app), Some("web-id"));
    }

    #[tokio::test]
    async fn start_stop_toggles_by_state() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        settle(&mut app, Message::Action(ActionId::StartStopSelected)).await;
        assert!(fake.calls().contains(&"stop:web-id".to_string()));

        app.containers.selected = Some("worker-id".into());
        settle(&mut app, Message::Action(ActionId::StartStopSelected)).await;
        assert!(fake.calls().contains(&"start:worker-id".to_string()));
    }

    #[tokio::test]
    async fn restart_selected_calls_the_client() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        settle(&mut app, Message::Action(ActionId::RestartSelected)).await;
        assert!(fake.calls().contains(&"restart:web-id".to_string()));
    }

    #[tokio::test]
    async fn container_actions_unavailable_without_connection_or_selection() {
        const SELECTED: [ActionId; 4] = [
            ActionId::OpenSelected,
            ActionId::StartStopSelected,
            ActionId::RestartSelected,
            ActionId::RemoveSelected,
        ];
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let calls_before = fake.calls().len();

        // No selection.
        app.containers.selected = None;
        for id in SELECTED {
            assert!(!actions::available(&app, id), "{id:?}");
            assert!(run(app.run_action(id)).await.is_empty());
        }
        // A selection that is not in the list.
        app.containers.selected = Some("gone-id".into());
        for id in SELECTED {
            assert!(!actions::available(&app, id), "{id:?}");
        }
        // Selection, but another screen.
        app.containers.selected = Some("web-id".into());
        app.screen = Screen::Volumes;
        for id in SELECTED.into_iter().chain([ActionId::SelectNext]) {
            assert!(!actions::available(&app, id), "{id:?}");
        }
        // Selection, but no Docker.
        app.screen = Screen::Containers;
        app.set_failed(DockerError::NotRunning);
        app.containers.selected = Some("web-id".into());
        for id in SELECTED.into_iter().chain([ActionId::SelectNext]) {
            assert!(!actions::available(&app, id), "{id:?}");
            let _ = app.run_action(id);
        }
        assert_eq!(app.containers.confirm_remove, None);
        assert_eq!(app.dock, None);
        assert_eq!(
            fake.calls().len(),
            calls_before,
            "the client was not called"
        );
        assert!(app.toasts.is_empty());
        assert!(app.containers.pending.is_empty());
    }

    #[tokio::test]
    async fn action_availability_of_the_rest() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for id in [
            ActionId::GoContainers,
            ActionId::GoVolumes,
            ActionId::GoNetworks,
            ActionId::ToggleSidebar,
            ActionId::ToggleDock,
            ActionId::FocusSearch,
            ActionId::OpenPalette,
            ActionId::ThemeSystem,
            ActionId::ThemeDark,
            ActionId::ThemeLight,
            ActionId::Escape,
        ] {
            assert!(actions::available(&app, id), "{id:?}");
        }
        assert!(
            !actions::available(&app, ActionId::Retry),
            "Retry needs a failure"
        );

        app.set_failed(DockerError::NotRunning);
        assert!(actions::available(&app, ActionId::Retry));
        assert!(actions::available(&app, ActionId::GoVolumes));
        assert!(actions::available(&app, ActionId::ThemeDark));
        assert!(!actions::available(&app, ActionId::ToggleDock));
        assert!(!actions::available(&app, ActionId::FocusSearch));
    }

    #[tokio::test]
    async fn remove_selected_opens_confirmation() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        let calls_before = fake.calls().len();
        settle(&mut app, Message::Action(ActionId::RemoveSelected)).await;
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("web-id"));
        assert_eq!(fake.calls().len(), calls_before, "nothing removed yet");
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove")));
    }

    #[tokio::test]
    async fn open_selected_opens_the_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("db-id".into());
        let _ = app.run_action(ActionId::OpenSelected);
        assert_eq!(
            app.dock.as_ref().map(|d| d.container_id.as_str()),
            Some("db-id")
        );
    }

    #[tokio::test]
    async fn escape_closes_top_layer_only() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.containers.query = "we".into();
        app.containers.confirm_remove = Some("web-id".into());
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert!(app.dock.is_some());

        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.containers.confirm_remove, None, "the modal goes first");
        assert!(app.dock.is_some());
        assert_eq!(app.containers.query, "we");

        let _ = app.run_action(ActionId::Escape);
        assert!(app.dock.is_none(), "then the dock");
        assert_eq!(app.containers.query, "we");

        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.containers.query, "", "then the search");

        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.containers.query, "");
        assert_eq!(selected_id(&app), Some("web-id"), "nothing else changes");
    }

    #[tokio::test]
    async fn escape_clearing_the_search_keeps_the_selection_visible() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("db-id".into());
        app.containers.query = "db".into();
        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.containers.query, "");
        assert_eq!(selected_id(&app), Some("db-id"));
    }

    #[tokio::test]
    async fn go_actions_navigate() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.run_action(ActionId::GoNetworks);
        assert_eq!(app.screen, Screen::Networks);
        let _ = app.run_action(ActionId::GoVolumes);
        assert_eq!(app.screen, Screen::Volumes);
        let _ = app.run_action(ActionId::GoContainers);
        assert_eq!(app.screen, Screen::Containers);
    }

    #[tokio::test]
    async fn layout_and_theme_actions() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.run_action(ActionId::ToggleSidebar);
        assert!(app.sidebar_collapsed);
        let _ = app.run_action(ActionId::ToggleSidebar);
        assert!(!app.sidebar_collapsed);

        app.containers.selected = Some("web-id".into());
        let _ = app.run_action(ActionId::ToggleDock);
        assert!(app.dock.is_some());
        let _ = app.run_action(ActionId::ToggleDock);
        assert!(app.dock.is_none());

        let _ = app.run_action(ActionId::ThemeDark);
        assert_eq!(app.theme_mode, ThemeMode::Dark);
        let _ = app.run_action(ActionId::ThemeLight);
        assert_eq!(app.theme_mode, ThemeMode::Light);
        let _ = app.run_action(ActionId::ThemeSystem);
        assert_eq!(app.theme_mode, ThemeMode::System);
    }

    #[tokio::test]
    async fn retry_action_pings_again() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let calls_before = fake.calls().len();
        // Connected: nothing to retry.
        assert!(run(app.run_action(ActionId::Retry)).await.is_empty());
        assert_eq!(fake.calls().len(), calls_before);

        app.set_failed(DockerError::NotRunning);
        settle(&mut app, Message::Action(ActionId::Retry)).await;
        assert!(fake.calls().contains(&"ping".to_string()));
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn keys_run_actions_through_the_keymap() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = None;
        let _ = app.update(key_msg("j", false));
        assert_eq!(selected_id(&app), Some("db-id"));

        let _ = app.update(Message::Key {
            key: iced::keyboard::Key::Character("2".into()),
            mods: iced::keyboard::Modifiers::CTRL,
            captured: false,
        });
        assert_eq!(app.screen, Screen::Volumes);
    }

    #[tokio::test]
    async fn typing_in_the_search_triggers_no_action() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        let calls_before = fake.calls().len();
        for c in ["s", "j", "k", "r", "/"] {
            let produced = run(app.update(key_msg(c, true))).await;
            assert!(produced.is_empty());
        }
        let _ = app.update(Message::Key {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete),
            mods: iced::keyboard::Modifiers::empty(),
            captured: true,
        });
        assert_eq!(fake.calls().len(), calls_before);
        assert_eq!(selected_id(&app), Some("web-id"));
        assert_eq!(app.containers.confirm_remove, None);
        assert!(app.containers.pending.is_empty());
    }

    #[tokio::test]
    async fn selected_actions_wait_while_the_removal_dialog_is_open() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.containers.confirm_remove = Some("web-id".into());
        for id in [
            ActionId::SelectNext,
            ActionId::StartStopSelected,
            ActionId::RestartSelected,
            ActionId::OpenSelected,
        ] {
            assert!(!actions::available(&app, id), "{id:?}");
        }
        assert!(actions::available(&app, ActionId::Escape));
    }

    #[test]
    fn keyboard_subscription_is_always_on() {
        let fake = Arc::new(FakeClient::new());
        let mut app = app_with(&fake);
        // The system theme and the keyboard, plus the connection's own subscription.
        for (conn, units) in [
            (Connection::Checking, 2),
            (Connection::Failed(DockerError::NotRunning), 3),
            (Connection::Ok, 3),
        ] {
            app.conn = conn;
            assert_eq!(app.subscription().units(), units);
        }
    }

    #[test]
    fn key_events_become_messages() {
        use iced::event::{Event, Status};
        use iced::keyboard;
        let pressed = Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("j".into()),
            modified_key: keyboard::Key::Character("j".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyJ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        });
        let id = iced::window::Id::unique();
        match key_event(pressed.clone(), Status::Captured, id) {
            Some(Message::Key { captured, .. }) => assert!(captured),
            other => panic!("{other:?}"),
        }
        match key_event(pressed, Status::Ignored, id) {
            Some(Message::Key { captured, .. }) => assert!(!captured),
            other => panic!("{other:?}"),
        }
        let released =
            Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL));
        assert!(key_event(released, Status::Ignored, id).is_none());
    }

    #[tokio::test]
    async fn enter_confirms_the_removal_dialog() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        // A stopped container: plain removal.
        app.containers.selected = Some("worker-id".into());
        settle(&mut app, Message::Action(ActionId::RemoveSelected)).await;
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("worker-id"));
        settle(&mut app, enter_key(true)).await;
        assert_eq!(app.containers.confirm_remove, None);
        assert!(fake.calls().contains(&"remove:worker-id".to_string()));
    }

    #[tokio::test]
    async fn enter_forces_the_removal_of_a_running_container() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        settle(&mut app, Message::Action(ActionId::RemoveSelected)).await;
        settle(&mut app, enter_key(false)).await;
        assert_eq!(app.containers.confirm_remove, None);
        assert!(fake.calls().contains(&"remove:web-id:force".to_string()));
    }

    #[tokio::test]
    async fn enter_without_the_dialog_does_not_remove() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        settle(&mut app, enter_key(false)).await;
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove")));
        assert!(app.dock.is_some(), "Enter still opens the dock");
    }

    fn enter_key(captured: bool) -> Message {
        Message::Key {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            mods: iced::keyboard::Modifiers::empty(),
            captured,
        }
    }

    #[tokio::test]
    async fn selecting_by_key_scrolls_the_row_into_view() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        // Room for one row only: the list is db, web, worker.
        let row = crate::ui::tokens::ROW_HEIGHT;
        app.containers.selected = Some("db-id".into());
        let _ = app.update(Message::Containers(containers::Msg::Scrolled {
            offset: 0.0,
            height: row,
        }));
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(app.containers.scroll_y, row, "web is the second row");
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(app.containers.scroll_y, 2.0 * row);
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(app.containers.scroll_y, row);
    }

    fn key(named: iced::keyboard::key::Named, captured: bool) -> Message {
        Message::Key {
            key: iced::keyboard::Key::Named(named),
            mods: iced::keyboard::Modifiers::empty(),
            captured,
        }
    }

    fn char_key(c: &str, mods: iced::keyboard::Modifiers, captured: bool) -> Message {
        Message::Key {
            key: iced::keyboard::Key::Character(c.into()),
            mods,
            captured,
        }
    }

    async fn open_palette(app: &mut App) {
        settle(app, Message::Action(ActionId::OpenPalette)).await;
        assert!(app.palette.is_some(), "the palette opens");
    }

    #[tokio::test]
    async fn open_palette_action_opens_it_empty() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert!(app.palette.is_none());
        open_palette(&mut app).await;
        assert_eq!(app.palette, Some(PaletteState::default()));
        // Opening again starts over.
        let _ = app.update(Message::PaletteQuery("vol".into()));
        open_palette(&mut app).await;
        assert_eq!(app.palette, Some(PaletteState::default()));
    }

    #[tokio::test]
    async fn palette_query_resets_the_cursor() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        open_palette(&mut app).await;
        let _ = app.update(Message::PaletteMove(3));
        assert_eq!(app.palette.as_ref().unwrap().cursor, 3);
        let _ = app.update(Message::PaletteQuery("vol".into()));
        let state = app.palette.as_ref().unwrap();
        assert_eq!(state.query, "vol");
        assert_eq!(state.cursor, 0);
    }

    #[tokio::test]
    async fn palette_run_executes_and_closes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        open_palette(&mut app).await;
        let _ = app.update(Message::PaletteQuery("volumes".into()));
        let _ = app.update(Message::PaletteRun(Some(ActionId::GoVolumes)));
        assert_eq!(app.palette, None);
        assert_eq!(app.screen, Screen::Volumes);
    }

    #[tokio::test]
    async fn palette_enter_without_results_keeps_open() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        open_palette(&mut app).await;
        let _ = app.update(Message::PaletteQuery("zzzzzz".into()));
        let calls_before = fake.calls().len();
        // The real key: Enter inside the text input arrives captured.
        let _ = app.update(key(iced::keyboard::key::Named::Enter, true));
        assert!(app.palette.is_some(), "the palette stays open");
        assert_eq!(app.screen, Screen::Containers);
        assert_eq!(fake.calls().len(), calls_before);
        let _ = app.update(Message::PaletteRun(None));
        assert!(app.palette.is_some());
    }

    #[tokio::test]
    async fn palette_cursor_is_clamped() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        open_palette(&mut app).await;
        let last = palette::matches(&app, "").len() - 1;
        let _ = app.update(Message::PaletteMove(50));
        assert_eq!(app.palette.as_ref().unwrap().cursor, last);
        let _ = app.update(Message::PaletteMove(-50));
        assert_eq!(app.palette.as_ref().unwrap().cursor, 0);
        // No results: the cursor stays at 0.
        let _ = app.update(Message::PaletteQuery("zzzzzz".into()));
        let _ = app.update(Message::PaletteMove(1));
        assert_eq!(app.palette.as_ref().unwrap().cursor, 0);
    }

    #[tokio::test]
    async fn palette_moves_and_runs_with_the_keys_even_when_captured() {
        use iced::keyboard::key::Named;
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        open_palette(&mut app).await;
        let _ = app.update(Message::PaletteQuery("volumes".into()));
        let _ = app.update(key(Named::ArrowDown, true));
        assert_eq!(app.palette.as_ref().unwrap().cursor, 0, "one result only");
        let _ = app.update(Message::PaletteQuery(String::new()));
        let _ = app.update(key(Named::ArrowDown, true));
        let _ = app.update(key(Named::ArrowDown, true));
        let _ = app.update(key(Named::ArrowUp, true));
        assert_eq!(app.palette.as_ref().unwrap().cursor, 1);
        // The selection of the list did not move.
        assert_eq!(selected_id(&app), Some("web-id"));
        let expected = palette::matches(&app, "")[1];
        assert_eq!(expected, ActionId::GoVolumes);
        let _ = app.update(key(Named::Enter, true));
        assert_eq!(app.palette, None);
        assert_eq!(app.screen, Screen::Volumes);
    }

    #[tokio::test]
    async fn palette_ignores_plain_keys_but_not_ctrl_shortcuts() {
        use iced::keyboard::Modifiers;
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        open_palette(&mut app).await;
        let calls_before = fake.calls().len();
        for c in ["s", "r", "j", "k", "/"] {
            let _ = app.update(char_key(c, Modifiers::empty(), false));
        }
        let _ = app.update(key(iced::keyboard::key::Named::Delete, false));
        assert_eq!(app.containers.confirm_remove, None);
        assert_eq!(fake.calls().len(), calls_before, "no action ran");
        assert_eq!(selected_id(&app), Some("web-id"));
        assert!(app.palette.is_some());

        let _ = app.update(char_key("2", Modifiers::CTRL, true));
        assert_eq!(app.screen, Screen::Volumes, "Ctrl shortcuts still work");
    }

    #[tokio::test]
    async fn palette_enter_wins_over_the_removal_dialog() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.confirm_remove = Some("web-id".into());
        open_palette(&mut app).await;
        let _ = app.update(Message::PaletteQuery("volumes".into()));
        let _ = app.update(key(iced::keyboard::key::Named::Enter, true));
        assert_eq!(app.screen, Screen::Volumes);
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove")));
        assert_eq!(
            app.containers.confirm_remove.as_deref(),
            Some("web-id"),
            "the dialog is still there"
        );
    }

    #[tokio::test]
    async fn escape_closes_palette_first() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.confirm_remove = Some("web-id".into());
        open_palette(&mut app).await;
        let _ = app.update(key(iced::keyboard::key::Named::Escape, true));
        assert_eq!(app.palette, None, "the palette goes first");
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("web-id"));
        let _ = app.update(key(iced::keyboard::key::Named::Escape, false));
        assert_eq!(app.containers.confirm_remove, None, "then the dialog");
    }

    #[tokio::test]
    async fn palette_runs_an_action_that_closes_it_before_running() {
        // Remove from the palette still asks for confirmation.
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        open_palette(&mut app).await;
        let _ = app.update(Message::PaletteRun(Some(ActionId::RemoveSelected)));
        assert_eq!(app.palette, None);
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("web-id"));
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove")));
    }

    #[test]
    fn palette_texts_are_translated() {
        for key in ["palette.placeholder", "palette.empty"] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(t!(key, locale = locale), key, "{locale} {key}");
            }
        }
    }

    #[tokio::test]
    async fn palette_view_is_drawn_over_the_window() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        open_palette(&mut app).await;
        let _ = app.view();
        app.palette = Some(PaletteState {
            query: "zzz".into(),
            cursor: 0,
        });
        let _ = app.view();
    }

    #[tokio::test]
    async fn theme_action_saves_preference() {
        let dir = crate::ui::prefs::tests::unique_dir("action");
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = App::new_with_config(fake.clone(), Some(dir.to_path_buf())).0;
        settle(&mut app, Message::Action(ActionId::ThemeDark)).await;
        assert_eq!(app.theme_mode, ThemeMode::Dark);
        assert_eq!(
            std::fs::read_to_string(dir.join("theme")).unwrap().trim(),
            "dark"
        );
        settle(&mut app, Message::Action(ActionId::ThemeLight)).await;
        assert_eq!(
            std::fs::read_to_string(dir.join("theme")).unwrap().trim(),
            "light"
        );
    }

    #[tokio::test]
    async fn saved_theme_is_loaded_at_startup() {
        let dir = crate::ui::prefs::tests::unique_dir("startup");
        crate::ui::prefs::save_theme_mode(&dir, ThemeMode::Light).unwrap();
        let fake = Arc::new(FakeClient::with_sample_data());
        let app = App::new_with_config(fake.clone(), Some(dir.to_path_buf())).0;
        assert_eq!(app.theme_mode, ThemeMode::Light);
    }

    #[tokio::test]
    async fn broken_theme_preference_opens_in_system_and_keeps_working() {
        let fake = Arc::new(FakeClient::with_sample_data());
        // Garbage in the file.
        let dir = crate::ui::prefs::tests::unique_dir("garbage");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("theme"), "azul").unwrap();
        let app = App::new_with_config(fake.clone(), Some(dir.to_path_buf())).0;
        assert_eq!(app.theme_mode, ThemeMode::System);
        // The config "directory" is a file: saving fails, quietly.
        let file = crate::ui::prefs::tests::unique_dir("not-a-dir");
        std::fs::write(&file, "x").unwrap();
        let mut app = App::new_with_config(fake.clone(), Some(file.to_path_buf())).0;
        assert_eq!(app.theme_mode, ThemeMode::System);
        settle(&mut app, Message::Action(ActionId::ThemeDark)).await;
        assert_eq!(app.theme_mode, ThemeMode::Dark, "the choice still applies");
        assert!(app.toasts.is_empty(), "a failure is only logged");
        // No directory at all: nothing is written anywhere.
        let mut app = App::new_with_config(fake.clone(), None).0;
        settle(&mut app, Message::Action(ActionId::ThemeLight)).await;
        assert_eq!(app.theme_mode, ThemeMode::Light);
    }

    #[tokio::test]
    async fn container_keys_do_nothing_while_the_dock_hides_the_list() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::ToggleMaximize);
        assert!(app.dock.as_ref().is_some_and(|d| d.maximized));
        let calls_before = fake.calls().len();
        for c in ["s", "r", "j", "k"] {
            settle(
                &mut app,
                char_key(c, iced::keyboard::Modifiers::empty(), false),
            )
            .await;
        }
        let _ = app.update(key(iced::keyboard::key::Named::Delete, false));
        assert_eq!(
            fake.calls().len(),
            calls_before,
            "the client was not called"
        );
        assert_eq!(app.containers.confirm_remove, None);
        assert_eq!(selected_id(&app), Some("web-id"));
    }

    #[tokio::test]
    async fn ctrl_shortcut_closes_the_palette_and_runs() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        open_palette(&mut app).await;
        let _ = app.update(char_key("2", iced::keyboard::Modifiers::CTRL, true));
        assert_eq!(app.palette, None);
        assert_eq!(app.screen, Screen::Volumes);
    }

    #[tokio::test]
    async fn removal_dialog_ignores_ctrl_shortcuts() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.confirm_remove = Some("web-id".into());
        let ctrl = iced::keyboard::Modifiers::CTRL;
        let _ = app.update(char_key("2", ctrl, false));
        let _ = app.update(char_key("b", ctrl, false));
        let _ = app.update(char_key(
            "p",
            ctrl | iced::keyboard::Modifiers::SHIFT,
            false,
        ));
        assert_eq!(app.screen, Screen::Containers);
        assert!(!app.sidebar_collapsed);
        assert!(app.palette.is_none());
        assert_eq!(app.containers.confirm_remove.as_deref(), Some("web-id"));
        // Esc still cancels.
        let _ = app.update(key(iced::keyboard::key::Named::Escape, false));
        assert_eq!(app.containers.confirm_remove, None);
    }

    #[tokio::test]
    async fn rapid_theme_changes_leave_the_last_one_saved() {
        let dir = crate::ui::prefs::tests::unique_dir("rapid");
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = App::new_with_config(fake, Some(dir.to_path_buf())).0;
        // The tasks are never run: the file is written by `run_action` itself.
        let _ = app.run_action(ActionId::ThemeDark);
        let _ = app.run_action(ActionId::ThemeLight);
        assert_eq!(
            std::fs::read_to_string(dir.join("theme")).unwrap().trim(),
            "light"
        );
    }
}
