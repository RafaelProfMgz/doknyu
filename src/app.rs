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
use crate::ui::prefs;
use crate::ui::style;
use crate::ui::theme::{ThemeMode, resolve};
use crate::ui::tokens::{BORDER_WIDTH, ROW_HEIGHT, SPACE_1};
use crate::views::containers::{self, ContainersState};
use crate::views::detail::{self, DetailState};
use crate::views::networks::{self, NetworksState};
use crate::views::palette::{self, PaletteState};
use crate::views::volumes::{self, VolumesState};
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
    Volumes(volumes::Msg),
    Networks(networks::Msg),
    /// Something in the container shown in the dock (logs today).
    Detail(detail::Msg),
    Docker(DockerEvent),
    EventsEnded,
    /// The wait after the events stream ended is over: check the daemon again.
    EventsRetry,
    /// The debounce window after a container event is over: reload the list.
    ReloadContainers,
    /// The debounce window after a volume event is over: reload the volumes.
    ReloadVolumes,
    /// The debounce window after a network event is over: reload the networks.
    ReloadNetworks,
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
    pub volumes: VolumesState,
    pub networks: NetworksState,
    pub toasts: Vec<Toast>,
    pub theme_mode: ThemeMode,
    /// Last theme reported by the system; `Mode::None` until it answers or has no preference.
    pub system_mode: iced::theme::Mode,
    pub sidebar_collapsed: bool,
    pub dock: Option<Dock>,
    /// What the dock tabs keep for the container it shows; exists exactly while `dock` does.
    pub detail: Option<DetailState>,
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
    /// Containers this user asked to remove: when they are gone the dock closes without saying
    /// so, since it was the user's own doing.
    removing: std::collections::HashSet<String>,
    /// A reload is waiting for the debounce window to end; further events join it.
    reload_scheduled: bool,
    /// Number of the latest list reload; answers from older ones are discarded.
    load_seq: u64,
    /// Same as `reload_scheduled` and `load_seq`, for the volume list.
    volumes_reload_scheduled: bool,
    volumes_load_seq: u64,
    /// Same again, for the network list.
    networks_reload_scheduled: bool,
    networks_load_seq: u64,
    /// When the current events subscription was opened.
    events_started: Option<tokio::time::Instant>,
    /// How long to wait the next time the events stream ends.
    events_backoff: Duration,
}

/// Row of a list of `len` rows that a step down (`forward`) or up from `current` lands on,
/// stopping at the ends. Without a current row, down picks the first and up the last; `None`
/// for an empty list.
fn step_target(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match (current, forward) {
        (None, true) => 0,
        (None, false) => last,
        (Some(i), true) => (i + 1).min(last),
        (Some(i), false) => i.saturating_sub(1),
    })
}

/// Scrolls the list `list_id` so row `index` is fully visible, if it is not. `scroll_y` is the
/// offset last reported by the list and is updated, since programmatic scrolling is not
/// reported back.
fn scroll_row_into_view(
    list_id: &'static str,
    index: usize,
    viewport_h: f32,
    scroll_y: &mut f32,
) -> Task<Message> {
    let Some(y) = containers::scroll_offset(index, ROW_HEIGHT, viewport_h, *scroll_y) else {
        return Task::none();
    };
    *scroll_y = y;
    iced::widget::operation::scroll_to(
        Id::new(list_id),
        iced::widget::operation::AbsoluteOffset {
            x: None,
            y: Some(y),
        },
    )
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
            volumes: VolumesState::default(),
            networks: NetworksState::default(),
            toasts: Vec::new(),
            theme_mode: config_dir
                .as_deref()
                .map(prefs::load_theme_mode)
                .unwrap_or_default(),
            system_mode: iced::theme::Mode::None,
            sidebar_collapsed: false,
            dock: None,
            detail: None,
            palette: None,
            palette_scroll_y: 0.0,
            config_dir,
            panes: initial_panes(),
            pinging: false,
            events_generation: 0,
            removing: std::collections::HashSet::new(),
            reload_scheduled: false,
            load_seq: 0,
            volumes_reload_scheduled: false,
            volumes_load_seq: 0,
            networks_reload_scheduled: false,
            networks_load_seq: 0,
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
                self.volumes.hovered = None;
                self.networks.hovered = None;
                Task::none()
            }
            Message::Pinged(Ok(())) => {
                self.conn = Connection::Ok;
                self.events_generation += 1;
                self.events_started = Some(tokio::time::Instant::now());
                Task::batch([
                    self.load_containers(),
                    self.load_volumes(),
                    self.load_networks(),
                ])
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
            Message::Volumes(m) => self.update_volumes(m),
            Message::ReloadVolumes => {
                self.volumes_reload_scheduled = false;
                if self.conn == Connection::Ok {
                    self.load_volumes()
                } else {
                    Task::none()
                }
            }
            Message::Networks(m) => self.update_networks(m),
            Message::ReloadNetworks => {
                self.networks_reload_scheduled = false;
                if self.conn == Connection::Ok {
                    self.load_networks()
                } else {
                    Task::none()
                }
            }
            Message::Detail(m) => match &mut self.detail {
                Some(detail) => detail.update(m),
                // The dock was closed while the message was on its way.
                None => Task::none(),
            },
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
                // Opening what is already shown keeps its tab, size and logs.
                if self.dock.as_ref().is_none_or(|d| d.container_id != id) {
                    self.detail = Some(DetailState::new(id.clone()));
                    self.dock = Some(Dock {
                        container_id: id,
                        tab: DockTab::Logs,
                        maximized: false,
                    });
                }
                Task::none()
            }
            Message::CloseDock => {
                self.close_dock();
                Task::none()
            }
            Message::ToggleDock => {
                if self.conn != Connection::Ok {
                    return Task::none();
                }
                match (&self.dock, self.containers.selected.clone()) {
                    (None, Some(id)) => self.update(Message::OpenDock(id)),
                    _ => {
                        self.close_dock();
                        Task::none()
                    }
                }
            }
            Message::DockTab(tab) => {
                let mut task = Task::none();
                if let Some(dock) = &mut self.dock
                    && dock.tab != tab
                {
                    dock.tab = tab;
                    // The tab is drawn again from scratch, with its list at the end.
                    if let Some(detail) = &mut self.detail {
                        detail.reset_scroll();
                        task = detail::scroll_list_to(0.0);
                    }
                }
                task
            }
            Message::ToggleMaximize => {
                let mut task = Task::none();
                if let Some(dock) = &mut self.dock {
                    dock.maximized = !dock.maximized;
                    if dock.maximized {
                        // The list is hidden, so the cursor exit of its row never arrives.
                        self.containers.hovered = None;
                    }
                    // The dock moves in the widget tree, which may start its lists over.
                    if let Some(detail) = &mut self.detail {
                        detail.reset_scroll();
                        task = detail::scroll_list_to(0.0);
                    }
                }
                task
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
            FocusSearch => iced::widget::operation::focus(Id::new(match self.screen {
                Screen::Volumes => volumes::SEARCH_ID,
                Screen::Networks => networks::SEARCH_ID,
                _ => containers::SEARCH_ID,
            })),
            FocusLogSearch => iced::widget::operation::focus(Id::new(detail::logs::SEARCH_ID)),
            DockLogs => match self.containers.selected.clone() {
                Some(id) => {
                    let open = self.update(Message::OpenDock(id));
                    Task::batch([open, self.update(Message::DockTab(DockTab::Logs))])
                }
                None => Task::none(),
            },
            DockStats => match self.containers.selected.clone() {
                Some(id) => {
                    let open = self.update(Message::OpenDock(id));
                    Task::batch([open, self.update(Message::DockTab(DockTab::Stats))])
                }
                None => Task::none(),
            },
            ToggleTimestamps => self.update(Message::Detail(detail::Msg::ToggleTimestamps)),
            CopyLogs => self.update(Message::Detail(detail::Msg::Copy)),
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
            RemoveSelected if self.screen == Screen::Volumes => {
                match self.volumes.selected.clone() {
                    Some(name) => self.update_volumes(volumes::Msg::AskRemove(name)),
                    None => Task::none(),
                }
            }
            RemoveSelected if self.screen == Screen::Networks => {
                match self.networks.selected.clone() {
                    Some(id) => self.update_networks(networks::Msg::AskRemove(id)),
                    None => Task::none(),
                }
            }
            RemoveSelected => on_selected(self, Msg::AskRemove),
            PruneVolumes => {
                // A maximized dock hides the list the dialog is about.
                if let Some(dock) = &mut self.dock {
                    dock.maximized = false;
                }
                let go = self.update(Message::Navigate(Screen::Volumes));
                Task::batch([go, self.update_volumes(volumes::Msg::AskPrune)])
            }
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

    /// The number the sidebar shows beside Volumes; none unless connected.
    fn volume_count(&self) -> Option<usize> {
        (self.conn == Connection::Ok).then_some(self.volumes.items.len())
    }

    /// The number the sidebar shows beside Networks; none unless connected.
    fn network_count(&self) -> Option<usize> {
        (self.conn == Connection::Ok).then_some(self.networks.items.len())
    }

    /// The selected container, if it is in the list.
    fn selected_container(&self) -> Option<&ContainerInfo> {
        let id = self.containers.selected.as_deref()?;
        self.containers.items.iter().find(|c| c.id == id)
    }

    /// Moves the selection one row down or up the visible list of the current screen, stopping
    /// at its ends. Without a selection, down picks the first row and up the last. Scrolls the
    /// list if the new row is not fully on screen.
    fn select_step(&mut self, forward: bool) -> Task<Message> {
        match self.screen {
            Screen::Containers => {
                let visible = self.containers.visible();
                let current = self
                    .containers
                    .selected
                    .as_deref()
                    .and_then(|id| visible.iter().position(|c| c.id == id));
                let Some(target) = step_target(visible.len(), current, forward) else {
                    return Task::none();
                };
                let id = visible[target].id.clone();
                self.containers.selected = Some(id);
                scroll_row_into_view(
                    containers::LIST_ID,
                    target,
                    self.containers.viewport_h,
                    &mut self.containers.scroll_y,
                )
            }
            Screen::Volumes => {
                let visible = self.volumes.visible();
                let current = self
                    .volumes
                    .selected
                    .as_deref()
                    .and_then(|name| visible.iter().position(|v| v.name == name));
                let Some(target) = step_target(visible.len(), current, forward) else {
                    return Task::none();
                };
                let name = visible[target].name.clone();
                self.volumes.selected = Some(name);
                scroll_row_into_view(
                    volumes::LIST_ID,
                    target,
                    self.volumes.viewport_h,
                    &mut self.volumes.scroll_y,
                )
            }
            Screen::Networks => {
                let visible = self.networks.visible();
                let current = self
                    .networks
                    .selected
                    .as_deref()
                    .and_then(|id| visible.iter().position(|n| n.id == id));
                let Some(target) = step_target(visible.len(), current, forward) else {
                    return Task::none();
                };
                let id = visible[target].id.clone();
                self.networks.selected = Some(id);
                scroll_row_into_view(
                    networks::LIST_ID,
                    target,
                    self.networks.viewport_h,
                    &mut self.networks.scroll_y,
                )
            }
        }
    }

    /// Esc closes one layer, the topmost: the command palette, then the removal dialog, then the
    /// dock (the selection in the logs first, then the log search, if it has text), then the
    /// search of the screen.
    fn close_top_layer(&mut self) {
        if self.palette.take().is_some() {
            return;
        }
        if self.containers.confirm_remove.take().is_some()
            || self.volumes.confirm.take().is_some()
            || self.networks.confirm_remove.take().is_some()
        {
            return;
        }
        // Selected text in the logs: Esc only deselects it.
        if let Some(detail) = &mut self.detail
            && self.dock.as_ref().is_some_and(|d| d.tab == DockTab::Logs)
            && detail.has_selection()
        {
            detail.clear_selection();
            return;
        }
        // Esc in the log search clears it first; the next one closes the dock. The app does not
        // know where the focus is, so a search text that is set counts as the focus being there.
        if let Some(detail) = &mut self.detail
            && self.dock.as_ref().is_some_and(|d| d.tab == DockTab::Logs)
            && !detail.query.is_empty()
        {
            detail.set_query(String::new());
            return;
        }
        if self.dock.is_some() {
            let _ = self.update(Message::CloseDock);
            return;
        }
        match self.screen {
            Screen::Containers if !self.containers.query.is_empty() => {
                self.containers.query.clear();
                self.containers.keep_selection_visible();
            }
            Screen::Volumes if !self.volumes.query.is_empty() => {
                self.volumes.query.clear();
                self.volumes.keep_selection_visible();
            }
            Screen::Networks if !self.networks.query.is_empty() => {
                self.networks.query.clear();
                self.networks.keep_selection_visible();
            }
            _ => {}
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
        let remove_force = containers::remove_force(&self.containers);
        let dialog_open = remove_force.is_some()
            || self.volumes.confirm.is_some()
            || self.networks.confirm_remove.is_some();
        if dialog_open
            && *key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
            && mods.is_empty()
        {
            return match remove_force {
                Some(force) => self.update_containers(containers::Msg::ConfirmRemove { force }),
                None if self.networks.confirm_remove.is_some() => {
                    self.update_networks(networks::Msg::Confirm)
                }
                // Does nothing if the dialog has nothing to confirm (nothing to prune).
                None => self.update_volumes(volumes::Msg::Confirm),
            };
        }
        let Some(id) = keymap::resolve(key, mods, captured) else {
            return Task::none();
        };
        let id = self.contextual(id, mods);
        // The dialog only answers to Enter and Esc; nothing else reaches the window behind it.
        if dialog_open && id != ActionId::Escape {
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

    /// The keymap does not know the app state, so Ctrl+F always comes as `FocusSearch`. With the
    /// dock open on the Logs tab it means the logs' search instead, also while the dock is
    /// maximized (the list is hidden then). `/` stays the list's search.
    fn contextual(&self, id: ActionId, mods: iced::keyboard::Modifiers) -> ActionId {
        let logs_open = self.dock.as_ref().is_some_and(|d| d.tab == DockTab::Logs);
        if id == ActionId::FocusSearch && mods.control() && logs_open {
            ActionId::FocusLogSearch
        } else {
            id
        }
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
                    self.close_dock();
                }
                // The container to remove is gone: nothing left to confirm.
                if let Some(id) = &self.containers.confirm_remove
                    && !self.containers.items.iter().any(|c| &c.id == id)
                {
                    self.containers.confirm_remove = None;
                }
                Task::none()
            }
            // The container of the dock is gone and the daemon says so.
            Msg::Loaded(_, Err(DockerError::NotFound(_))) if self.dock.is_some() => {
                self.close_gone_dock();
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
                    Err(e) => {
                        // It is still there, so a later removal from outside is news.
                        self.removing.remove(&id);
                        self.fail(e)
                    }
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

    fn update_volumes(&mut self, m: volumes::Msg) -> Task<Message> {
        use volumes::{Msg, VolumeConfirm};
        match m {
            // Reloads can overlap: only the latest one counts.
            Msg::Loaded(seq, _) if seq != self.volumes_load_seq => Task::none(),
            Msg::Loaded(_, Ok(items)) => {
                self.volumes.set_items(items);
                Task::none()
            }
            Msg::Loaded(_, Err(e)) => self.fail(e),
            Msg::Query(query) => {
                self.volumes.query = query;
                self.volumes.keep_selection_visible();
                Task::none()
            }
            Msg::Select(name) => {
                self.volumes.selected = Some(name);
                Task::none()
            }
            Msg::Hover(name) => {
                self.volumes.hovered = Some(name);
                Task::none()
            }
            Msg::Leave(name) => {
                if self.volumes.hovered.as_deref() == Some(name.as_str()) {
                    self.volumes.hovered = None;
                }
                Task::none()
            }
            // A volume in use is not offered for removal: Docker would refuse it anyway.
            Msg::AskRemove(name) => {
                let removable = self
                    .volumes
                    .items
                    .iter()
                    .any(|v| v.name == name && v.used_by.is_empty())
                    && !self.volumes.pending.contains(&name);
                if removable {
                    self.volumes.confirm = Some(VolumeConfirm::Remove(name));
                }
                Task::none()
            }
            Msg::AskPrune => {
                self.volumes.confirm = Some(VolumeConfirm::Prune(self.volumes.unused()));
                Task::none()
            }
            Msg::Confirm => {
                if !self.volumes.can_confirm() {
                    return Task::none();
                }
                match self.volumes.confirm.take() {
                    Some(VolumeConfirm::Remove(name)) => self.remove_volume(name),
                    // Exactly the volumes the dialog listed, none else: Docker's own prune would
                    // also take one that became free after the dialog opened.
                    Some(VolumeConfirm::Prune(names)) => self.prune_volumes(names),
                    None => Task::none(),
                }
            }
            Msg::Cancel => {
                self.volumes.confirm = None;
                Task::none()
            }
            // The list is refreshed by the Docker events the removal causes.
            Msg::Removed(name, result) => {
                self.volumes.pending.remove(&name);
                match result {
                    Ok(()) => Task::none(),
                    Err(e) => self.fail(e),
                }
            }
            Msg::Pruned { removed, failed } => {
                for (name, _) in &failed {
                    self.volumes.pending.remove(name);
                }
                for name in &removed {
                    self.volumes.pending.remove(name);
                }
                // The daemon is gone: the onboarding explains it, no need for more toasts.
                if let Some((_, e)) = failed.iter().find(|(_, e)| e.is_connection()) {
                    return self.fail(e.clone());
                }
                if !removed.is_empty() || failed.is_empty() {
                    let text = if removed.len() == 1 {
                        t!("volumes.pruned_one", count = 1)
                    } else {
                        t!("volumes.pruned", count = removed.len())
                    };
                    self.push_toast_of(ToastKind::Info, text.into_owned(), None);
                }
                // A volume that became used since the dialog opened is refused and stays.
                if let Some((_, e)) = failed.first() {
                    let names: Vec<&str> = failed.iter().map(|(n, _)| n.as_str()).collect();
                    let details = format!("{}: {e:?}", names.join(", "));
                    self.push_toast(friendly_error(e), Some(details));
                }
                Task::none()
            }
            Msg::Scrolled { offset, height } => {
                self.volumes.scroll_y = offset;
                self.volumes.viewport_h = height;
                Task::none()
            }
            Msg::Resized(height) => {
                self.volumes.viewport_h = height;
                Task::none()
            }
        }
    }

    /// Removes a volume unless a removal of it is already running.
    fn remove_volume(&mut self, name: String) -> Task<Message> {
        if !self.volumes.pending.insert(name.clone()) {
            return Task::none();
        }
        let client = self.client.clone();
        Task::perform(
            async move {
                let result = client.remove_volume(&name).await;
                (name, result)
            },
            |(name, result)| Message::Volumes(volumes::Msg::Removed(name, result)),
        )
    }

    /// Removes the volumes one by one, without force, so one that is in use again is refused by
    /// Docker and stays. The outcome comes back as a single `Pruned`.
    fn prune_volumes(&mut self, names: Vec<String>) -> Task<Message> {
        let names: Vec<String> = names
            .into_iter()
            .filter(|n| self.volumes.pending.insert(n.clone()))
            .collect();
        // Everything listed is already being removed: nothing to do, nothing to report.
        if names.is_empty() {
            return Task::none();
        }
        let client = self.client.clone();
        Task::perform(
            async move {
                let mut removed = Vec::new();
                let mut failed = Vec::new();
                for name in names {
                    match client.remove_volume(&name).await {
                        Ok(()) => removed.push(name),
                        Err(e) => failed.push((name, e)),
                    }
                }
                (removed, failed)
            },
            |(removed, failed)| Message::Volumes(volumes::Msg::Pruned { removed, failed }),
        )
    }

    fn load_networks(&mut self) -> Task<Message> {
        self.networks_load_seq += 1;
        let seq = self.networks_load_seq;
        let client = self.client.clone();
        Task::perform(async move { client.list_networks().await }, move |r| {
            Message::Networks(networks::Msg::Loaded(seq, r))
        })
    }

    fn update_networks(&mut self, m: networks::Msg) -> Task<Message> {
        use networks::Msg;
        match m {
            // Reloads can overlap: only the latest one counts.
            Msg::Loaded(seq, _) if seq != self.networks_load_seq => Task::none(),
            Msg::Loaded(_, Ok(items)) => {
                self.networks.set_items(items);
                Task::none()
            }
            Msg::Loaded(_, Err(e)) => self.fail(e),
            Msg::Query(query) => {
                self.networks.query = query;
                self.networks.keep_selection_visible();
                Task::none()
            }
            Msg::Select(id) => {
                self.networks.selected = Some(id);
                Task::none()
            }
            Msg::Hover(id) => {
                self.networks.hovered = Some(id);
                Task::none()
            }
            Msg::Leave(id) => {
                if self.networks.hovered.as_deref() == Some(id.as_str()) {
                    self.networks.hovered = None;
                }
                Task::none()
            }
            // Docker's default networks are never offered. One with containers connected is: the
            // dialog warns, and Docker answers `Conflict` if they are still there.
            Msg::AskRemove(id) => {
                if self.networks.removable(&id) {
                    self.networks.confirm_remove = Some(id);
                }
                Task::none()
            }
            Msg::Confirm => match self.networks.confirm_remove.take() {
                Some(id) if self.networks.removable(&id) => self.remove_network(id),
                _ => Task::none(),
            },
            Msg::Cancel => {
                self.networks.confirm_remove = None;
                Task::none()
            }
            // The list is refreshed by the Docker events the removal causes.
            Msg::Removed(id, result) => {
                self.networks.pending.remove(&id);
                match result {
                    Ok(()) => Task::none(),
                    Err(e) => self.fail(e),
                }
            }
            Msg::Scrolled { offset, height } => {
                self.networks.scroll_y = offset;
                self.networks.viewport_h = height;
                Task::none()
            }
            Msg::Resized(height) => {
                self.networks.viewport_h = height;
                Task::none()
            }
        }
    }

    /// Removes a network unless a removal of it is already running.
    fn remove_network(&mut self, id: String) -> Task<Message> {
        if !self.networks.pending.insert(id.clone()) {
            return Task::none();
        }
        let client = self.client.clone();
        Task::perform(
            async move {
                let result = client.remove_network(&id).await;
                (id, result)
            },
            |(id, result)| Message::Networks(networks::Msg::Removed(id, result)),
        )
    }

    fn load_volumes(&mut self) -> Task<Message> {
        self.volumes_load_seq += 1;
        let seq = self.volumes_load_seq;
        let client = self.client.clone();
        Task::perform(async move { client.list_volumes().await }, move |r| {
            Message::Volumes(volumes::Msg::Loaded(seq, r))
        })
    }

    /// Container events refresh the container list, volume events the volume list, network events
    /// the network list; each is
    /// debounced so a burst causes a single reload. A container created or removed also
    /// changes who uses each volume, so it refreshes the volumes too.
    fn on_docker_event(&mut self, ev: DockerEvent) -> Task<Message> {
        // The stream is alive.
        self.events_backoff = EVENTS_BACKOFF_MIN;
        let mut rescroll = Task::none();
        // The shown container was removed: nothing left to show. The reload that follows
        // finds it missing from the list and has nothing more to say.
        if ev.kind == ResourceKind::Container
            && ev.action == "destroy"
            && self.dock.as_ref().is_some_and(|d| d.container_id == ev.id)
        {
            self.close_gone_dock();
        }
        if ev.kind == ResourceKind::Container && ev.action == "destroy" {
            self.removing.remove(&ev.id);
        }
        // The shown container runs again: its streams ended with the old run, so it gets new
        // ones (a new generation), with the logs starting over.
        if ev.kind == ResourceKind::Container
            // `docker restart` says `restart` and then `start`: one new run, one resubscription.
            && ev.action == "start"
            && let Some(detail) = &mut self.detail
            && detail.id == ev.id
        {
            detail.resubscribe();
            rescroll = detail::scroll_list_to(0.0);
        }
        let mut tasks = vec![rescroll];
        if ev.kind == ResourceKind::Container && !self.reload_scheduled {
            self.reload_scheduled = true;
            tasks.push(Task::perform(tokio::time::sleep(RELOAD_DEBOUNCE), |()| {
                Message::ReloadContainers
            }));
        }
        let changes_volumes = ev.kind == ResourceKind::Volume
            || (ev.kind == ResourceKind::Container
                && matches!(ev.action.as_str(), "create" | "destroy"));
        if changes_volumes && !self.volumes_reload_scheduled {
            self.volumes_reload_scheduled = true;
            tasks.push(Task::perform(tokio::time::sleep(RELOAD_DEBOUNCE), |()| {
                Message::ReloadVolumes
            }));
        }
        // Docker reports a container joining or leaving a network as a network event; one
        // created or removed also changes who is connected, so that reloads the networks too.
        let changes_networks = ev.kind == ResourceKind::Network
            || (ev.kind == ResourceKind::Container
                && matches!(ev.action.as_str(), "create" | "destroy"));
        if changes_networks && !self.networks_reload_scheduled {
            self.networks_reload_scheduled = true;
            tasks.push(Task::perform(tokio::time::sleep(RELOAD_DEBOUNCE), |()| {
                Message::ReloadNetworks
            }));
        }
        Task::batch(tasks)
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
        self.volumes.confirm = None;
        self.volumes.hovered = None;
        self.networks.confirm_remove = None;
        self.networks.hovered = None;
        self.close_dock();
    }

    /// The container in the dock was removed: closes the dock and says so. A stream that ends
    /// is not this (the container may only have stopped); only `destroy` or a `NotFound` is.
    /// When the user removed it here, there is nothing to say.
    fn close_gone_dock(&mut self) {
        let by_user = self
            .dock
            .as_ref()
            .is_some_and(|d| self.removing.remove(&d.container_id));
        self.close_dock();
        if by_user {
            return;
        }
        self.push_toast_of(ToastKind::Warning, t!("dock.gone").into_owned(), None);
    }

    /// Closes the dock and forgets what its tabs kept.
    fn close_dock(&mut self) {
        self.dock = None;
        self.detail = None;
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
        if matches!(action, ContainerAction::Remove { .. }) {
            self.removing.insert(id.clone());
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
        self.push_toast_of(ToastKind::Error, text, details);
    }

    fn push_toast_of(&mut self, kind: ToastKind, text: String, details: Option<String>) {
        if self.toasts.iter().any(|t| t.text == text) {
            return;
        }
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast {
            kind,
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
        let volumes = self.volume_count();
        let networks = self.network_count();
        let body = column![
            row![
                sidebar::view(
                    &self.screen,
                    self.sidebar_collapsed,
                    count,
                    volumes,
                    networks
                ),
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
        let body = match volumes::dialog(&self.volumes) {
            Some(card) => modal(body, card, Message::Volumes(volumes::Msg::Cancel)),
            None => body,
        };
        let body = match networks::dialog(&self.networks) {
            Some(card) => modal(body, card, Message::Networks(networks::Msg::Cancel)),
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
        let dock_view = || {
            dock::view(
                open,
                self.container_info(&open.container_id),
                self.detail.as_ref(),
            )
        };
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

    /// A container of the list, by id.
    fn container_info(&self, id: &str) -> Option<&ContainerInfo> {
        self.containers.items.iter().find(|c| c.id == id)
    }

    fn screen_content(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::Containers => containers::view(&self.containers),
            Screen::Volumes => volumes::view(&self.volumes),
            Screen::Networks => networks::view(&self.networks),
        }
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
        let logs = self
            .logs_source()
            .map_or_else(Subscription::none, detail::logs::subscription);
        let stats = self
            .stats_source()
            .map_or_else(Subscription::none, detail::stats::subscription);
        Subscription::batch([system_theme, conn, keys, logs, stats])
    }

    /// The log stream wanted right now: only with Docker reachable, the dock open and its
    /// container still in the list. A stopped container counts: its history is worth showing,
    /// and the stream ends by itself.
    fn logs_source(&self) -> Option<detail::logs::LogsSource> {
        let detail = self.detail.as_ref()?;
        let listed = self.containers.items.iter().any(|c| c.id == detail.id);
        (self.conn == Connection::Ok && listed).then(|| detail::logs::LogsSource {
            client: self.client.clone(),
            id: detail.id.clone(),
            generation: detail.generation,
        })
    }

    /// The stats stream wanted right now: like the logs, but only while the container runs
    /// (a stopped one has no consumption). It runs whichever tab is on screen on purpose: the
    /// history keeps filling, so the user finds the last 60 s already there when opening
    /// Stats. When the container runs again the list says so and the generation is new,
    /// which starts a new stream.
    fn stats_source(&self) -> Option<detail::stats::StatsSource> {
        let detail = self.detail.as_ref()?;
        let running = self
            .container_info(&detail.id)
            .is_some_and(|c| c.state.is_running());
        (self.conn == Connection::Ok && running).then(|| detail::stats::StatsSource {
            client: self.client.clone(),
            id: detail.id.clone(),
            generation: detail.generation,
        })
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
        // Selection, but another screen: Volumes and Networks have their own selection (and no
        // container actions).
        app.containers.selected = Some("web-id".into());
        for screen in [Screen::Volumes, Screen::Networks] {
            app.screen = screen;
            for id in SELECTED {
                assert!(!actions::available(&app, id), "{id:?}");
            }
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
        let _ = app.update(Message::PaletteQuery("go to vol".into()));
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

    // ---- Logs tab (task 3.2) ----

    fn log_line(text: &str) -> LogLine {
        LogLine {
            stream: crate::model::LogStream::Stdout,
            timestamp: None,
            text: text.into(),
        }
    }

    fn log_texts(app: &App) -> Vec<String> {
        app.detail
            .as_ref()
            .map(|d| d.logs.iter().map(|l| l.text.clone()).collect())
            .unwrap_or_default()
    }

    fn batch_msg(id: &str, generation: u64, texts: &[&str]) -> Message {
        Message::Detail(detail::Msg::LogBatch {
            id: id.into(),
            generation,
            lines: texts.iter().map(|t| log_line(t)).collect(),
        })
    }

    #[tokio::test]
    async fn the_detail_lives_as_long_as_the_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert!(app.detail.is_none());
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(app.detail.as_ref().map(|d| d.id.as_str()), Some("web-id"));

        let _ = app.update(Message::CloseDock);
        assert!(app.detail.is_none());

        app.containers.selected = Some("web-id".into());
        let _ = app.update(Message::ToggleDock);
        assert!(app.detail.is_some());
        let _ = app.update(Message::ToggleDock);
        assert!(app.detail.is_none());

        let _ = app.update(Message::OpenDock("web-id".into()));
        app.set_failed(DockerError::NotRunning);
        assert!(app.detail.is_none(), "no Docker: nothing to show");

        app.conn = Connection::Ok;
        let _ = app.update(Message::OpenDock("web-id".into()));
        fake.remove("web-id", true).await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert!(app.dock.is_none() && app.detail.is_none());
    }

    #[tokio::test]
    async fn log_batch_appends() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["a", "b"]));
        let _ = app.update(batch_msg("web-id", 0, &["c"]));
        assert_eq!(log_texts(&app), ["a", "b", "c"]);
        // Nothing open: a late batch is just dropped.
        let _ = app.update(Message::CloseDock);
        let _ = app.update(batch_msg("web-id", 0, &["late"]));
        assert!(app.detail.is_none());
    }

    #[tokio::test]
    async fn scroll_up_disables_follow() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let scrolled = |from_bottom| {
            Message::Detail(detail::Msg::Scrolled {
                from_bottom,
                viewport_h: 300.0,
            })
        };
        let _ = app.update(scrolled(200.0));
        assert_eq!(app.detail.as_ref().map(|d| d.follow), Some(false));
        let _ = app.update(scrolled(0.0));
        assert_eq!(app.detail.as_ref().map(|d| d.follow), Some(true));
    }

    #[tokio::test]
    async fn switching_container_resets_buffer() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["a"]));
        let _ = app.update(Message::Detail(detail::Msg::Query("a".into())));
        assert_eq!(log_texts(&app), ["a"]);

        // Opening the same container again changes nothing.
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(log_texts(&app), ["a"]);

        let _ = app.update(Message::OpenDock("db-id".into()));
        let detail = app.detail.as_ref().unwrap();
        assert_eq!(detail.id, "db-id");
        assert!(detail.logs.is_empty());
        assert!(detail.query.is_empty() && detail.matches.is_empty());
        // The old container's late lines do not leak in.
        let _ = app.update(batch_msg("web-id", 0, &["stale"]));
        assert!(log_texts(&app).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn restart_resubscribes_logs() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.push_log("web-id", log_line("old"));
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let client: Arc<dyn DockerClient> = fake.clone();

        let source = app.logs_source().expect("the dock is open");
        assert_eq!(source.generation, 0);
        let mut first = Box::pin(detail::logs::log_messages(
            client.clone(),
            source.id,
            source.generation,
        ));
        let _ = app.update(first.next().await.unwrap());
        assert_eq!(log_texts(&app), ["old"]);

        // The container stops: its stream ends, the detail stays with what it has.
        fake.stop("web-id").await.unwrap();
        let _ = app.update(first.next().await.unwrap());
        assert!(first.next().await.is_none());
        assert!(app.detail.as_ref().unwrap().logs_ended);
        assert_eq!(log_texts(&app), ["old"]);
        assert!(app.dock.is_some());

        // It starts again: a new generation, an empty buffer, and the history comes back once.
        fake.start("web-id").await.unwrap();
        let _ = app.update(Message::Docker(container_event("start")));
        let detail = app.detail.as_ref().unwrap();
        assert_eq!(detail.generation, 1);
        assert!(detail.logs.is_empty() && !detail.logs_ended);
        // A batch still on its way from the old stream is ignored.
        let _ = app.update(batch_msg("web-id", 0, &["old"]));
        assert!(log_texts(&app).is_empty());

        let source = app.logs_source().unwrap();
        assert_eq!(source.generation, 1);
        let mut second = Box::pin(detail::logs::log_messages(
            client,
            source.id,
            source.generation,
        ));
        fake.push_log("web-id", log_line("new"));
        let _ = app.update(second.next().await.unwrap());
        assert_eq!(log_texts(&app), ["old", "new"], "history once, then live");
    }

    #[tokio::test]
    async fn restart_events_resubscribe_once() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        // `docker restart` makes the daemon say `restart` and `start`.
        let _ = app.update(Message::Docker(container_event("restart")));
        let _ = app.update(Message::Docker(container_event("start")));
        assert_eq!(app.detail.as_ref().unwrap().generation, 1);
    }

    #[tokio::test]
    async fn logs_resume_after_leaving_a_selection_on_another_tab() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["a"]));
        let _ = app.update(Message::Detail(detail::Msg::Editor(
            iced::widget::text_editor::Action::SelectAll,
        )));
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.update(Message::DockTab(DockTab::Logs));
        let _ = app.update(batch_msg("web-id", 0, &["b"]));
        let d = app.detail.as_ref().unwrap();
        assert_eq!(d.editor_text(), "a\nb");
        assert!(d.follow);
    }

    #[tokio::test]
    async fn events_of_other_containers_or_kinds_keep_the_logs() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["a"]));
        let other = DockerEvent {
            id: "db-id".into(),
            ..container_event("start")
        };
        let _ = app.update(Message::Docker(other));
        let _ = app.update(Message::Docker(container_event("die")));
        let volume = DockerEvent {
            kind: ResourceKind::Volume,
            ..container_event("start")
        };
        let _ = app.update(Message::Docker(volume));
        assert_eq!(app.detail.as_ref().unwrap().generation, 0);
        assert_eq!(log_texts(&app), ["a"]);
    }

    #[tokio::test]
    async fn logs_are_followed_only_with_the_dock_open_on_an_existing_container() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert!(app.logs_source().is_none(), "no dock");
        let _ = app.subscription();

        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(
            app.logs_source().map(|s| (s.id, s.generation)),
            Some(("web-id".into(), 0))
        );
        let _ = app.subscription();

        // The log tab does not have to be on screen: the buffer keeps filling.
        let _ = app.update(Message::DockTab(DockTab::Stats));
        assert!(app.logs_source().is_some());

        // A stopped container still has history worth showing.
        fake.stop("web-id").await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert!(app.logs_source().is_some());

        // The container left the list: no stream for it.
        app.containers.items.retain(|c| c.id != "web-id");
        assert!(app.logs_source().is_none());

        let _ = app.update(Message::CloseDock);
        assert!(app.logs_source().is_none());
    }

    #[tokio::test]
    async fn no_logs_subscription_without_docker() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert!(app.logs_source().is_none());
    }

    #[tokio::test]
    async fn escape_clears_the_log_search_before_closing_the_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["error here"]));
        let _ = app.update(Message::Detail(detail::Msg::Query("error".into())));
        app.containers.query = "we".into();

        let _ = app.run_action(ActionId::Escape);
        assert!(
            app.dock.is_some(),
            "the first Esc only clears the log search"
        );
        assert!(app.detail.as_ref().unwrap().query.is_empty());
        assert!(app.detail.as_ref().unwrap().matches.is_empty());
        assert_eq!(app.containers.query, "we");

        let _ = app.run_action(ActionId::Escape);
        assert!(app.dock.is_none(), "the next one closes the dock");
        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.containers.query, "");
    }

    #[tokio::test]
    async fn escape_clears_log_selection_first() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["error here", "and there"]));
        let _ = app.update(Message::Detail(detail::Msg::Query("error".into())));
        let _ = app.update(Message::Detail(detail::Msg::Editor(
            iced::widget::text_editor::Action::SelectAll,
        )));
        assert!(app.detail.as_ref().unwrap().has_selection());

        let _ = app.run_action(ActionId::Escape);
        assert!(
            app.dock.is_some(),
            "the first Esc only clears the selection"
        );
        assert!(!app.detail.as_ref().unwrap().has_selection());
        assert_eq!(
            app.detail.as_ref().unwrap().query,
            "error",
            "the search text is still there"
        );

        let _ = app.run_action(ActionId::Escape);
        assert!(app.detail.as_ref().unwrap().query.is_empty());
        assert!(app.dock.is_some());
        let _ = app.run_action(ActionId::Escape);
        assert!(app.dock.is_none());
    }

    #[tokio::test]
    async fn the_log_search_is_not_cleared_from_other_tabs() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::Detail(detail::Msg::Query("x".into())));
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.run_action(ActionId::Escape);
        assert!(app.dock.is_none(), "Esc closes the dock from the Stats tab");
    }

    #[tokio::test]
    async fn ctrl_f_searches_the_logs_when_the_dock_shows_them() {
        let ctrl = iced::keyboard::Modifiers::CTRL;
        let none = iced::keyboard::Modifiers::empty();
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert_eq!(
            app.contextual(ActionId::FocusSearch, ctrl),
            ActionId::FocusSearch
        );

        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(
            app.contextual(ActionId::FocusSearch, ctrl),
            ActionId::FocusLogSearch
        );
        // `/` is the list's search whenever the list is on screen.
        assert_eq!(
            app.contextual(ActionId::FocusSearch, none),
            ActionId::FocusSearch
        );
        // Maximized: the list is hidden, so Ctrl+F still goes to the logs.
        let _ = app.update(Message::ToggleMaximize);
        assert_eq!(
            app.contextual(ActionId::FocusSearch, ctrl),
            ActionId::FocusLogSearch
        );
        let _ = app.update(Message::ToggleMaximize);

        let _ = app.update(Message::DockTab(DockTab::Stats));
        assert_eq!(
            app.contextual(ActionId::FocusSearch, ctrl),
            ActionId::FocusSearch
        );
        // Other actions are never rewritten.
        assert_eq!(
            app.contextual(ActionId::ToggleDock, ctrl),
            ActionId::ToggleDock
        );
    }

    #[tokio::test]
    async fn ctrl_f_key_press_is_handled_with_the_dock_open() {
        let ctrl = iced::keyboard::Modifiers::CTRL;
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        // Both ways of getting there end in an operation, not a state change or a panic.
        let _ = run(app.update(char_key("f", ctrl, false))).await;
        let _ = run(app.update(char_key("f", ctrl, true))).await;
        assert!(app.dock.is_some());
    }

    #[tokio::test]
    async fn dock_logs_opens_the_selected_container_on_the_logs_tab() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = None;
        assert!(!actions::available(&app, ActionId::DockLogs));

        app.containers.selected = Some("web-id".into());
        let _ = app.run_action(ActionId::DockLogs);
        assert_eq!(app.dock, Some(web_dock()));

        // Already open on another tab: it goes to Logs.
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.run_action(ActionId::DockLogs);
        assert_eq!(app.dock.as_ref().map(|d| d.tab), Some(DockTab::Logs));

        // Another selection: the dock follows it.
        app.containers.selected = Some("db-id".into());
        let _ = app.run_action(ActionId::DockLogs);
        assert_eq!(app.detail.as_ref().map(|d| d.id.as_str()), Some("db-id"));

        // The shortcut reaches it, also from the maximized dock (the selection is still there).
        let _ = app.update(Message::CloseDock);
        let _ = app.update(char_key(
            "L",
            iced::keyboard::Modifiers::CTRL | iced::keyboard::Modifiers::SHIFT,
            false,
        ));
        assert_eq!(
            app.dock.as_ref().map(|d| d.container_id.as_str()),
            Some("db-id")
        );
    }

    #[tokio::test]
    async fn dock_logs_needs_docker() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.set_failed(DockerError::NotRunning);
        assert!(!actions::available(&app, ActionId::DockLogs));
        let _ = app.run_action(ActionId::DockLogs);
        assert!(app.dock.is_none());
    }

    #[tokio::test]
    async fn log_actions_need_the_logs_tab_on_screen() {
        const LOG_ACTIONS: [ActionId; 3] = [
            ActionId::ToggleTimestamps,
            ActionId::CopyLogs,
            ActionId::FocusLogSearch,
        ];
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for id in LOG_ACTIONS {
            assert!(!actions::available(&app, id), "{id:?} without a dock");
        }
        let _ = app.update(Message::OpenDock("web-id".into()));
        assert!(actions::available(&app, ActionId::ToggleTimestamps));
        assert!(actions::available(&app, ActionId::FocusLogSearch));
        assert!(
            !actions::available(&app, ActionId::CopyLogs),
            "nothing to copy yet"
        );
        let _ = app.update(batch_msg("web-id", 0, &["a"]));
        assert!(actions::available(&app, ActionId::CopyLogs));

        let _ = app.update(Message::DockTab(DockTab::Stats));
        for id in LOG_ACTIONS {
            assert!(!actions::available(&app, id), "{id:?} on the Stats tab");
        }
    }

    #[tokio::test]
    async fn toggle_timestamps_action_flips_the_option() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.run_action(ActionId::ToggleTimestamps);
        assert_eq!(app.detail.as_ref().map(|d| d.timestamps), Some(true));
        let _ = app.run_action(ActionId::ToggleTimestamps);
        assert_eq!(app.detail.as_ref().map(|d| d.timestamps), Some(false));
    }

    #[tokio::test]
    async fn the_list_goes_back_to_the_end_when_it_is_drawn_again() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let scroll_up = || {
            Message::Detail(detail::Msg::Scrolled {
                from_bottom: 90.0,
                viewport_h: 200.0,
            })
        };
        let _ = app.update(scroll_up());
        // Other tab and back: the scrollable is new, at its end.
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.update(Message::DockTab(DockTab::Logs));
        assert_eq!(app.detail.as_ref().map(|d| d.follow), Some(true));

        // Maximizing moves the dock in the widget tree.
        let _ = app.update(scroll_up());
        let _ = app.update(Message::ToggleMaximize);
        assert_eq!(app.detail.as_ref().map(|d| d.follow), Some(true));

        // Clicking the tab that is already open does not move the list.
        let _ = app.update(scroll_up());
        let _ = app.update(Message::DockTab(DockTab::Logs));
        assert_eq!(app.detail.as_ref().map(|d| d.follow), Some(false));
    }

    #[tokio::test]
    async fn view_builds_with_live_logs_in_every_layout() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let texts: Vec<String> = (0..500).map(|i| format!("line {i}")).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let _ = app.update(batch_msg("web-id", 0, &refs));
        let _ = app.update(Message::Detail(detail::Msg::Query("line 4".into())));
        for tab in [DockTab::Logs, DockTab::Stats, DockTab::Terminal] {
            let _ = app.update(Message::DockTab(tab));
            for _ in 0..2 {
                let _ = app.view();
                let _ = app.update(Message::ToggleMaximize);
            }
        }
    }

    // ---- Stats tab (task 3.3) ----

    fn raw_stats(mem: u64) -> RawStats {
        RawStats {
            cpu_total: 0,
            precpu_total: 0,
            system: 0,
            presystem: 0,
            online_cpus: 2,
            mem_usage: mem,
            mem_inactive_file: 0,
            mem_limit: 1000,
            net_rx: 1,
            net_tx: 2,
            blk_read: 3,
            blk_write: 4,
        }
    }

    fn mem_series(app: &App) -> Vec<f64> {
        app.detail
            .as_ref()
            .map(|d| d.stats.mem_series())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn stats_samples_reach_the_history_of_the_open_container_only() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let sample = |id: &str, generation, mem| {
            Message::Detail(detail::Msg::StatsSample {
                id: id.into(),
                generation,
                raw: raw_stats(mem),
            })
        };
        // The dock closed while the sample was on its way: dropped quietly.
        let _ = app.update(sample("web-id", 0, 1));
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(sample("web-id", 0, 10));
        let _ = app.update(sample("db-id", 0, 99));
        let _ = app.update(sample("web-id", 5, 99));
        assert_eq!(mem_series(&app), [10.0]);
    }

    #[tokio::test]
    async fn stopped_container_has_no_stats_subscription() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert!(app.stats_source().is_none(), "no dock");

        let _ = app.update(Message::OpenDock("web-id".into()));
        assert_eq!(
            app.stats_source().map(|s| (s.id, s.generation)),
            Some(("web-id".into(), 0))
        );
        let _ = app.subscription();

        // The stream is wanted whichever tab is on screen, so the history is there when the
        // user gets to Stats.
        let _ = app.update(Message::DockTab(DockTab::Stats));
        assert!(app.stats_source().is_some());

        fake.stop("web-id").await.unwrap();
        settle(&mut app, Message::ReloadContainers).await;
        assert!(app.stats_source().is_none(), "stopped");
        assert!(app.logs_source().is_some(), "the logs stay");
        let _ = app.subscription();

        // A container that was never running neither.
        let _ = app.update(Message::OpenDock("worker-id".into()));
        assert!(app.stats_source().is_none());

        let _ = app.update(Message::OpenDock("db-id".into()));
        assert!(app.stats_source().is_some());
        app.containers.items.retain(|c| c.id != "db-id");
        assert!(app.stats_source().is_none(), "gone from the list");
        app.containers = containers::ContainersState::default();
        let _ = app.update(Message::CloseDock);
        assert!(app.stats_source().is_none());
    }

    #[tokio::test]
    async fn no_stats_subscription_without_docker() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert!(app.stats_source().is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn restart_resubscribes_stats() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let client: Arc<dyn DockerClient> = fake.clone();

        let source = app.stats_source().expect("running");
        let mut first = Box::pin(detail::stats::stats_messages(
            client.clone(),
            source.id,
            source.generation,
        ));
        fake.push_stats("web-id", raw_stats(10));
        let _ = app.update(first.next().await.unwrap());
        assert_eq!(mem_series(&app), [10.0]);

        // The container stops: the stream ends; the dock and the last numbers stay.
        fake.stop("web-id").await.unwrap();
        let _ = app.update(first.next().await.unwrap());
        assert!(first.next().await.is_none());
        assert!(app.detail.as_ref().unwrap().stats_ended);
        assert!(app.dock.is_some());

        // It runs again: a new generation and a history that starts over.
        fake.start("web-id").await.unwrap();
        let _ = app.update(Message::Docker(container_event("start")));
        assert!(mem_series(&app).is_empty());
        assert!(!app.detail.as_ref().unwrap().stats_ended);
        settle(&mut app, Message::ReloadContainers).await;
        let source = app.stats_source().expect("running again");
        assert_eq!(source.generation, 1);
        let mut second = Box::pin(detail::stats::stats_messages(
            client,
            source.id,
            source.generation,
        ));
        fake.push_stats("web-id", raw_stats(20));
        let _ = app.update(second.next().await.unwrap());
        // A sample of the old stream, still on its way, is ignored.
        let _ = app.update(Message::Detail(detail::Msg::StatsSample {
            id: "web-id".into(),
            generation: 0,
            raw: raw_stats(10),
        }));
        assert_eq!(mem_series(&app), [20.0]);
    }

    #[tokio::test]
    async fn stopped_container_keeps_detail_open() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.push_log("web-id", log_line("kept"));
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(batch_msg("web-id", 0, &["kept"]));

        fake.stop("web-id").await.unwrap();
        // The streams end, then the daemon's events reload the list.
        for m in [
            detail::Msg::LogsEnded {
                id: "web-id".into(),
                generation: 0,
            },
            detail::Msg::StatsEnded {
                id: "web-id".into(),
                generation: 0,
            },
        ] {
            let _ = app.update(Message::Detail(m));
        }
        settle(&mut app, Message::Docker(container_event("die"))).await;
        settle(&mut app, Message::Docker(container_event("stop"))).await;

        assert_eq!(app.screen, Screen::Containers);
        assert!(app.dock.is_some(), "the dock stays open");
        assert_eq!(log_texts(&app), ["kept"], "with the logs it had");
        assert!(app.toasts.is_empty(), "nothing is said about a stop");
        let web = app.containers.items.iter().find(|c| c.id == "web-id");
        assert!(web.is_some_and(|c| !c.state.is_running()));
        let _ = app.view();
    }

    #[tokio::test]
    async fn destroyed_container_returns_to_list() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::DockTab(DockTab::Stats));
        let _ = app.update(batch_msg("web-id", 0, &["a"]));

        // Removed from outside: both streams end, then the daemon says `destroy`.
        fake.close_streams("web-id");
        for m in [
            detail::Msg::LogsEnded {
                id: "web-id".into(),
                generation: 0,
            },
            detail::Msg::StatsEnded {
                id: "web-id".into(),
                generation: 0,
            },
        ] {
            let _ = app.update(Message::Detail(m));
        }
        let _ = app.update(Message::Docker(container_event("destroy")));

        assert_eq!(app.screen, Screen::Containers);
        assert!(app.dock.is_none() && app.detail.is_none());
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].text, t!("dock.gone"));
        assert_eq!(app.conn, Connection::Ok, "not a connection problem");
        // The reload that follows does not say it again.
        settle(&mut app, Message::ReloadContainers).await;
        assert_eq!(app.toasts.len(), 1);
        let _ = app.view();
    }

    #[tokio::test]
    async fn removing_the_container_here_closes_the_dock_without_the_gone_toast() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.containers.confirm_remove = Some("web-id".into());
        let _ = app.update(Message::Containers(containers::Msg::ConfirmRemove {
            force: true,
        }));
        // The action can finish before the event that says it is gone.
        let _ = app.update(Message::Containers(containers::Msg::ActionDone(
            "web-id".into(),
            Ok(()),
        )));
        let _ = app.update(Message::Docker(container_event("destroy")));
        assert!(app.dock.is_none() && app.detail.is_none());
        assert!(app.toasts.is_empty(), "the user knows: {:?}", app.toasts);
    }

    #[tokio::test]
    async fn a_failed_removal_does_not_hide_a_later_external_one() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.containers.confirm_remove = Some("web-id".into());
        let _ = app.update(Message::Containers(containers::Msg::ConfirmRemove {
            force: true,
        }));
        let _ = app.update(Message::Containers(containers::Msg::ActionDone(
            "web-id".into(),
            Err(DockerError::Conflict("busy".into())),
        )));
        app.toasts.clear();
        let _ = app.update(Message::Docker(container_event("destroy")));
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].text, t!("dock.gone"));
    }

    #[tokio::test]
    async fn destroying_another_container_leaves_the_dock_alone() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        // Nothing is open: nothing to say.
        let _ = app.update(Message::Docker(container_event("destroy")));
        assert!(app.toasts.is_empty());

        let _ = app.update(Message::OpenDock("web-id".into()));
        let other = DockerEvent {
            id: "db-id".into(),
            ..container_event("destroy")
        };
        let _ = app.update(Message::Docker(other));
        let volume = DockerEvent {
            kind: ResourceKind::Volume,
            ..container_event("destroy")
        };
        let _ = app.update(Message::Docker(volume));
        assert!(app.dock.is_some() && app.toasts.is_empty());
    }

    #[tokio::test]
    async fn not_found_on_reload_with_the_dock_open_says_the_container_is_gone() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.load_seq += 1;
        let seq = app.load_seq;
        let _ = app.update(Message::Containers(containers::Msg::Loaded(
            seq,
            Err(DockerError::NotFound("No such container: web-id".into())),
        )));
        assert!(app.dock.is_none());
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].text, t!("dock.gone"));
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn not_found_on_reload_without_a_dock_keeps_the_usual_error() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.load_seq += 1;
        let seq = app.load_seq;
        let _ = app.update(Message::Containers(containers::Msg::Loaded(
            seq,
            Err(DockerError::NotFound("x".into())),
        )));
        assert_eq!(app.toasts.len(), 1);
        assert_ne!(app.toasts[0].text, t!("dock.gone"));
    }

    #[tokio::test]
    async fn dock_stats_opens_the_selected_container_on_the_stats_tab() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        app.containers.selected = None;
        assert!(!actions::available(&app, ActionId::DockStats));

        app.containers.selected = Some("web-id".into());
        assert!(actions::available(&app, ActionId::DockStats));
        let _ = app.run_action(ActionId::DockStats);
        assert_eq!(app.dock.as_ref().map(|d| d.tab), Some(DockTab::Stats));
        assert_eq!(app.detail.as_ref().map(|d| d.id.as_str()), Some("web-id"));

        // Already open on the logs: it goes to Stats; another selection: the dock follows.
        let _ = app.update(Message::DockTab(DockTab::Logs));
        let _ = app.run_action(ActionId::DockStats);
        assert_eq!(app.dock.as_ref().map(|d| d.tab), Some(DockTab::Stats));
        app.containers.selected = Some("db-id".into());
        let _ = app.run_action(ActionId::DockStats);
        assert_eq!(app.detail.as_ref().map(|d| d.id.as_str()), Some("db-id"));
        assert_eq!(app.dock.as_ref().map(|d| d.tab), Some(DockTab::Stats));

        // The shortcut reaches it.
        let _ = app.update(Message::CloseDock);
        let _ = app.update(char_key(
            "S",
            iced::keyboard::Modifiers::CTRL | iced::keyboard::Modifiers::SHIFT,
            false,
        ));
        assert_eq!(app.dock.as_ref().map(|d| d.tab), Some(DockTab::Stats));

        app.set_failed(DockerError::NotRunning);
        assert!(!actions::available(&app, ActionId::DockStats));
    }

    #[tokio::test]
    async fn view_builds_the_stats_tab_running_stopped_and_empty() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for id in ["web-id", "worker-id"] {
            let _ = app.update(Message::OpenDock(id.into()));
            let _ = app.update(Message::DockTab(DockTab::Stats));
            let _ = app.view(); // waiting for the first sample (or stopped)
            for mem in [0, 100, 900] {
                let _ = app.update(Message::Detail(detail::Msg::StatsSample {
                    id: id.into(),
                    generation: 0,
                    raw: raw_stats(mem),
                }));
                for _ in 0..2 {
                    let _ = app.view();
                    let _ = app.update(Message::ToggleMaximize);
                }
            }
        }
    }

    #[test]
    fn stats_texts_are_translated() {
        for key in [
            "dock.gone",
            "actions.dock_stats",
            "stats.cpu",
            "stats.memory",
            "stats.network",
            "stats.disk",
            "stats.rx",
            "stats.tx",
            "stats.read",
            "stats.write",
            "stats.of_limit",
            "stats.stopped",
            "stats.waiting",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(key, locale = locale, used = 1, limit = 2, cpus = 3),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }

    // ---- Volumes screen (task 4.1) ----

    fn volume_event(action: &str, name: &str) -> DockerEvent {
        DockerEvent {
            kind: ResourceKind::Volume,
            action: action.into(),
            id: name.into(),
        }
    }

    fn volume_list_calls(fake: &FakeClient) -> usize {
        fake.calls().iter().filter(|c| *c == "list_volumes").count()
    }

    fn vol_msg(m: volumes::Msg) -> Message {
        Message::Volumes(m)
    }

    /// Connected, on the Volumes screen: `db_data` (used by `db`) and `cache` (unused).
    async fn volumes_app(fake: &Arc<FakeClient>) -> App {
        let mut app = loaded_app(fake).await;
        let _ = app.run_action(ActionId::GoVolumes);
        assert_eq!(app.screen, Screen::Volumes);
        app
    }

    fn named_key(named: iced::keyboard::key::Named) -> Message {
        key(named, false)
    }

    #[tokio::test]
    async fn connecting_loads_the_volumes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let app = loaded_app(&fake).await;
        assert_eq!(app.volumes.items.len(), 2);
        assert_eq!(volume_list_calls(&fake), 1);
    }

    #[tokio::test]
    async fn in_use_volume_cannot_be_removed() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskRemove("db_data".into())));
        assert_eq!(app.volumes.confirm, None);

        // Neither does the Delete key or the palette action offer it.
        app.volumes.selected = Some("db_data".into());
        assert!(!actions::available(&app, ActionId::RemoveSelected));
        settle(&mut app, key(iced::keyboard::key::Named::Delete, false)).await;
        assert_eq!(app.volumes.confirm, None);
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_volume")));
    }

    #[tokio::test]
    async fn unused_volume_is_removed_after_confirming() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskRemove("cache".into())));
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Remove("cache".into()))
        );
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_volume")));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert!(fake.calls().contains(&"remove_volume:cache".to_string()));
        assert_eq!(app.volumes.confirm, None);
        assert!(app.volumes.pending.is_empty());
        assert!(app.toasts.is_empty());
        // The daemon's `destroy` event refreshes the list.
        settle(&mut app, Message::Docker(volume_event("destroy", "cache"))).await;
        let names: Vec<_> = app.volumes.items.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["db_data"]);
    }

    #[tokio::test]
    async fn remove_conflict_keeps_item_and_toasts() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskRemove("cache".into())));
        fake.fail_next(DockerError::Conflict("volume is in use".into()));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert_eq!(app.volumes.items.len(), 2, "the volume is still listed");
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(
            app.toasts[0].text,
            friendly_error(&DockerError::Conflict(String::new()))
        );
        assert!(app.volumes.pending.is_empty(), "the row is usable again");
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn removing_a_volume_twice_runs_one_call() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskRemove("cache".into())));
        let first = app.update(vol_msg(volumes::Msg::Confirm));
        // A second dialog for the same volume while the first removal is in flight.
        let _ = app.update(vol_msg(volumes::Msg::AskRemove("cache".into())));
        assert_eq!(
            app.volumes.confirm, None,
            "busy volumes are not asked about"
        );
        let _ = run(first).await;
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|c| *c == "remove_volume:cache")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn prune_dialog_lists_only_unused() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.add_volume(crate::model::VolumeInfo {
            name: "tmp".into(),
            driver: "local".into(),
            mountpoint: "/var/lib/docker/volumes/tmp/_data".into(),
            used_by: vec![],
        });
        let mut app = loaded_app(&fake).await;
        let _ = app.run_action(ActionId::GoVolumes);
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec![
                "cache".into(),
                "tmp".into()
            ]))
        );
        let _ = app.view();
        assert!(!fake.calls().contains(&"prune_volumes".to_string()));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert!(fake.calls().contains(&"remove_volume:cache".to_string()));
        assert_eq!(app.volumes.confirm, None);
    }

    #[tokio::test]
    async fn prune_with_nothing_unused_does_not_call_docker() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.remove_volume("cache").await.unwrap();
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec![]))
        );
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        settle(&mut app, enter_key(false)).await;
        assert!(!fake.calls().contains(&"prune_volumes".to_string()));
        assert!(app.volumes.confirm.is_some(), "the dialog stays open");
        let _ = app.view();
    }

    fn unused_volume(name: &str) -> crate::model::VolumeInfo {
        crate::model::VolumeInfo {
            name: name.into(),
            driver: "local".into(),
            mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
            used_by: vec![],
        }
    }

    fn removed_volumes(fake: &FakeClient) -> Vec<String> {
        fake.calls()
            .into_iter()
            .filter(|c| c.starts_with("remove_volume:"))
            .collect()
    }

    #[tokio::test]
    async fn prune_dialog_follows_reloads() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec!["cache".into()]))
        );
        // `db` goes away, so `db_data` is free by the time the list is reloaded.
        fake.remove("db-id", true).await.unwrap();
        settle(&mut app, Message::ReloadVolumes).await;
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec![
                "cache".into(),
                "db_data".into()
            ]))
        );
    }

    #[tokio::test]
    async fn prune_removes_only_listed_volumes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        // Unused, but it showed up after the dialog opened: it was never listed.
        fake.add_volume(unused_volume("tmp"));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert_eq!(removed_volumes(&fake), vec!["remove_volume:cache"]);
        assert!(!fake.calls().contains(&"prune_volumes".to_string()));
        let names: Vec<_> = fake
            .list_volumes()
            .await
            .unwrap()
            .into_iter()
            .map(|v| v.name)
            .collect();
        assert!(names.contains(&"tmp".to_string()));
    }

    #[tokio::test]
    async fn prune_keeps_volumes_that_became_used() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.add_volume(unused_volume("tmp"));
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec![
                "cache".into(),
                "tmp".into()
            ]))
        );
        // The first removal (`cache`) is refused, as if a container had just mounted it.
        fake.fail_next(DockerError::Conflict("volume is in use".into()));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert_eq!(
            removed_volumes(&fake),
            vec!["remove_volume:cache", "remove_volume:tmp"]
        );
        let error = app.toasts.iter().find(|t| t.kind == ToastKind::Error);
        let error = error.expect("an error toast");
        assert_eq!(
            error.text,
            friendly_error(&DockerError::Conflict(String::new()))
        );
        assert!(
            error
                .details
                .as_deref()
                .is_some_and(|d| d.contains("cache"))
        );
        let info = app.toasts.iter().find(|t| t.kind == ToastKind::Info);
        assert_eq!(
            info.map(|t| t.text.clone()),
            Some(t!("volumes.pruned_one", count = 1).into_owned())
        );
        assert!(app.volumes.pending.is_empty());
        settle(&mut app, Message::ReloadVolumes).await;
        let mut names: Vec<_> = app.volumes.items.iter().map(|v| v.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["cache", "db_data"]);
    }

    #[tokio::test]
    async fn prune_action_restores_a_maximized_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        let _ = app.update(Message::ToggleMaximize);
        assert!(app.dock.as_ref().is_some_and(|d| d.maximized));
        settle(&mut app, Message::Action(ActionId::PruneVolumes)).await;
        assert_eq!(app.screen, Screen::Volumes);
        assert!(app.volumes.confirm.is_some());
        let dock = app.dock.as_ref().expect("the dock stays open");
        assert!(
            !dock.maximized,
            "the list must be visible behind the dialog"
        );
        let _ = app.view();
    }

    #[tokio::test]
    async fn prune_with_every_listed_volume_pending_does_nothing() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        app.volumes.pending.insert("cache".into());
        let calls = fake.calls().len();
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert_eq!(fake.calls().len(), calls, "no call to Docker");
        assert!(app.toasts.is_empty(), "no \"0 removed\" toast");
        assert!(
            app.volumes.pending.contains("cache"),
            "the other removal keeps its mark"
        );
    }

    #[tokio::test]
    async fn prune_dialog_does_not_list_volumes_being_removed() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.volumes.pending.insert("cache".into());
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec![]))
        );
        assert!(!app.volumes.can_confirm());
    }

    #[tokio::test]
    async fn remove_dialog_closes_when_the_volume_vanishes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskRemove("cache".into())));
        assert!(app.volumes.confirm.is_some());
        // Removed from outside; the event leads to a reload.
        fake.remove_volume("cache").await.unwrap();
        let calls = fake.calls().len();
        settle(&mut app, Message::ReloadVolumes).await;
        assert_eq!(app.volumes.confirm, None);
        let after_reload = fake.calls().len();
        assert!(after_reload > calls);
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        settle(&mut app, enter_key(false)).await;
        assert_eq!(fake.calls().len(), after_reload, "no call to Docker");
        assert!(app.toasts.is_empty(), "no 404 toast");
    }

    #[tokio::test]
    async fn prune_result_toast_counts() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let text = |n: usize| {
            if n == 1 {
                t!("volumes.pruned_one", count = n).into_owned()
            } else {
                t!("volumes.pruned", count = n).into_owned()
            }
        };
        for n in [3usize, 1, 0] {
            let removed = (0..n).map(|i| format!("v{i}")).collect();
            let _ = app.update(vol_msg(volumes::Msg::Pruned {
                removed,
                failed: vec![],
            }));
            let last = app.toasts.last().expect("a toast");
            assert_eq!(last.text, text(n));
            assert_eq!(last.kind, ToastKind::Info);
            assert!(last.text.contains(&n.to_string()));
        }
        assert_eq!(app.toasts.len(), 3);
    }

    #[tokio::test]
    async fn prune_from_the_dialog_toasts_the_removed_count() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(
            app.toasts[0].text,
            t!("volumes.pruned_one", count = 1).into_owned()
        );
        settle(&mut app, Message::Docker(volume_event("destroy", "cache"))).await;
        assert_eq!(app.volumes.items.len(), 1);
    }

    #[tokio::test]
    async fn prune_failure_toasts_the_error() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        fake.fail_next(DockerError::Other("boom".into()));
        settle(&mut app, vol_msg(volumes::Msg::Confirm)).await;
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].kind, ToastKind::Error);
        assert_eq!(app.volumes.items.len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn volume_event_triggers_reload() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = volume_list_calls(&fake);
        fake.remove_volume("cache").await.unwrap();
        settle(&mut app, Message::Docker(volume_event("destroy", "cache"))).await;
        assert_eq!(volume_list_calls(&fake), before + 1);
        assert_eq!(app.volumes.items.len(), 1);
        assert_eq!(list_calls(&fake), 1, "containers are not reloaded for it");
    }

    #[tokio::test(start_paused = true)]
    async fn burst_of_volume_events_reloads_once() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = volume_list_calls(&fake);
        let tasks: Vec<_> = (0..5)
            .map(|_| app.update(Message::Docker(volume_event("destroy", "x"))))
            .collect();
        let mut produced = Vec::new();
        for task in tasks {
            produced.extend(run(task).await);
        }
        assert_eq!(produced.len(), 1, "only the first event schedules a reload");
        for m in produced {
            settle(&mut app, m).await;
        }
        assert_eq!(volume_list_calls(&fake), before + 1);
        // The window is over: the next event schedules again.
        settle(&mut app, Message::Docker(volume_event("create", "y"))).await;
        assert_eq!(volume_list_calls(&fake), before + 2);
    }

    #[tokio::test(start_paused = true)]
    async fn container_create_and_destroy_refresh_who_uses_each_volume() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for action in ["create", "destroy"] {
            let before = volume_list_calls(&fake);
            settle(&mut app, Message::Docker(container_event(action))).await;
            assert_eq!(volume_list_calls(&fake), before + 1, "{action}");
        }
        // A stop does not change which volumes are mounted.
        let before = volume_list_calls(&fake);
        settle(&mut app, Message::Docker(container_event("die"))).await;
        assert_eq!(volume_list_calls(&fake), before);
    }

    #[tokio::test(start_paused = true)]
    async fn volume_reload_is_skipped_when_docker_is_unreachable() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Docker(volume_event("destroy", "x")));
        app.conn = Connection::Failed(DockerError::NotRunning);
        let before = volume_list_calls(&fake);
        assert!(run(app.update(Message::ReloadVolumes)).await.is_empty());
        assert_eq!(volume_list_calls(&fake), before);
        // The flag was cleared: a later event can schedule again.
        app.conn = Connection::Ok;
        let produced = run(app.update(Message::Docker(volume_event("create", "y")))).await;
        assert_eq!(produced.len(), 1);
    }

    #[tokio::test]
    async fn an_older_volume_load_cannot_overwrite_a_newer_one() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let stale = app.volumes_load_seq;
        settle(&mut app, Message::ReloadVolumes).await;
        assert!(app.volumes_load_seq > stale);
        let _ = app.update(vol_msg(volumes::Msg::Loaded(stale, Ok(vec![]))));
        assert_eq!(app.volumes.items.len(), 2);
        let _ = app.update(vol_msg(volumes::Msg::Loaded(
            stale,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn volume_load_connection_error_goes_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.volumes.confirm = Some(volumes::VolumeConfirm::Prune(vec![]));
        let seq = app.volumes_load_seq;
        let _ = app.update(vol_msg(volumes::Msg::Loaded(
            seq,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert_eq!(app.volumes.confirm, None);
        let _ = app.view();
    }

    #[tokio::test]
    async fn volume_keyboard_selection_moves_in_the_visible_list() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        // Visible order is by name: cache, db_data.
        assert_eq!(app.volumes.selected, None);
        let _ = app.update(key_msg("j", false));
        assert_eq!(app.volumes.selected.as_deref(), Some("cache"));
        let _ = app.update(named_key(iced::keyboard::key::Named::ArrowDown));
        assert_eq!(app.volumes.selected.as_deref(), Some("db_data"));
        let _ = app.update(key_msg("j", false));
        assert_eq!(app.volumes.selected.as_deref(), Some("db_data"), "no wrap");
        let _ = app.update(key_msg("k", false));
        assert_eq!(app.volumes.selected.as_deref(), Some("cache"));
        let _ = app.update(key_msg("k", false));
        assert_eq!(app.volumes.selected.as_deref(), Some("cache"), "no wrap");
        assert_eq!(app.containers.selected, None, "containers are untouched");
        app.volumes.selected = None;
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(app.volumes.selected.as_deref(), Some("db_data"));
    }

    #[tokio::test]
    async fn volume_selection_scrolls_the_row_into_view() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let row = crate::ui::tokens::ROW_HEIGHT;
        app.volumes.selected = Some("cache".into());
        let _ = app.update(vol_msg(volumes::Msg::Scrolled {
            offset: 0.0,
            height: row,
        }));
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(app.volumes.scroll_y, row, "db_data is the second row");
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(app.volumes.scroll_y, 0.0);
    }

    #[tokio::test]
    async fn delete_enter_and_escape_drive_the_volume_dialog() {
        use iced::keyboard::key::Named;
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.volumes.selected = Some("cache".into());
        settle(&mut app, key(Named::Delete, false)).await;
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Remove("cache".into()))
        );
        let _ = app.view();

        // Esc cancels and removes nothing.
        settle(&mut app, key(Named::Escape, false)).await;
        assert_eq!(app.volumes.confirm, None);
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_volume")));

        // Behind the dialog the keys do nothing but Enter and Esc.
        settle(&mut app, key(Named::Delete, false)).await;
        let _ = app.update(key_msg("j", false));
        let _ = app.update(key_msg("2", false));
        assert_eq!(app.volumes.selected.as_deref(), Some("cache"));
        assert!(!actions::available(&app, ActionId::SelectNext));

        // Enter confirms, even if a text input holds the event.
        settle(&mut app, enter_key(true)).await;
        assert_eq!(app.volumes.confirm, None);
        assert!(fake.calls().contains(&"remove_volume:cache".to_string()));
    }

    #[tokio::test]
    async fn enter_on_the_prune_dialog_prunes() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::AskPrune));
        settle(&mut app, enter_key(false)).await;
        assert!(fake.calls().contains(&"remove_volume:cache".to_string()));
        assert_eq!(app.volumes.confirm, None);
    }

    #[tokio::test]
    async fn enter_on_volumes_does_not_open_the_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.volumes.selected = Some("cache".into());
        for id in [
            ActionId::OpenSelected,
            ActionId::StartStopSelected,
            ActionId::RestartSelected,
        ] {
            assert!(!actions::available(&app, id), "{id:?}");
        }
        settle(&mut app, enter_key(false)).await;
        assert!(app.dock.is_none());
        assert!(!fake.calls().iter().any(|c| c.starts_with("start")));
    }

    #[tokio::test]
    async fn escape_on_volumes_closes_layers_in_order() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.volumes.query = "ca".into();
        app.containers.query = "we".into();
        app.volumes.selected = Some("cache".into());
        app.volumes.confirm = Some(volumes::VolumeConfirm::Remove("cache".into()));
        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.volumes.confirm, None, "the dialog goes first");
        assert_eq!(app.volumes.query, "ca");
        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.volumes.query, "", "then the search");
        assert_eq!(
            app.containers.query, "we",
            "the other screen's search stays"
        );
        assert_eq!(app.volumes.selected.as_deref(), Some("cache"));
    }

    #[tokio::test]
    async fn search_filters_volumes_and_keeps_the_selection_visible() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.volumes.selected = Some("db_data".into());
        let _ = app.update(vol_msg(volumes::Msg::Query("cach".into())));
        assert_eq!(app.volumes.selected.as_deref(), Some("cache"));
        let _ = app.view();
    }

    #[tokio::test]
    async fn volume_search_is_focusable_only_on_the_volumes_screen() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        assert!(actions::available(&app, ActionId::FocusSearch));
        app.screen = Screen::Networks;
        // Networks have a search and a list of their own now.
        assert!(actions::available(&app, ActionId::FocusSearch));
        assert!(actions::available(&app, ActionId::SelectNext));
    }

    #[tokio::test]
    async fn remove_selected_on_volumes_opens_the_volume_dialog_not_the_container_one() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.volumes.selected = Some("cache".into());
        settle(&mut app, Message::Action(ActionId::RemoveSelected)).await;
        assert_eq!(app.containers.confirm_remove, None);
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Remove("cache".into()))
        );
    }

    #[tokio::test]
    async fn prune_action_is_a_palette_action_without_shortcut() {
        let def = actions::def(ActionId::PruneVolumes);
        assert!(def.in_palette);
        assert_eq!(def.shortcut, None);
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert!(palette::matches(&app, "prune").contains(&ActionId::PruneVolumes));
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert!(!actions::available(&app, ActionId::PruneVolumes));
        assert!(palette::matches(&app, "prune").is_empty());
    }

    #[tokio::test]
    async fn prune_action_from_containers_goes_to_volumes_and_asks() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert_eq!(app.screen, Screen::Containers);
        // From the palette, which closes before running it.
        let _ = app.update(Message::Action(ActionId::OpenPalette));
        settle(&mut app, Message::PaletteRun(Some(ActionId::PruneVolumes))).await;
        assert!(app.palette.is_none());
        assert_eq!(app.screen, Screen::Volumes);
        assert_eq!(
            app.volumes.confirm,
            Some(volumes::VolumeConfirm::Prune(vec!["cache".into()]))
        );
        assert!(!fake.calls().contains(&"prune_volumes".to_string()));
        let _ = app.view();
    }

    #[tokio::test]
    async fn sidebar_counts_the_volumes_while_connected() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert_eq!(app.volume_count(), Some(2));
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert_eq!(app.volume_count(), None);
        app.conn = Connection::Checking;
        assert_eq!(app.volume_count(), None);
        let _ = app.view();
    }

    #[tokio::test]
    async fn volumes_view_builds_with_dock_dialog_and_toasts() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.volumes.selected = Some("cache".into());
        app.volumes.hovered = Some("db_data".into());
        app.volumes.pending.insert("cache".into());
        for confirm in [
            None,
            Some(volumes::VolumeConfirm::Remove("cache".into())),
            Some(volumes::VolumeConfirm::Prune(vec!["cache".into()])),
        ] {
            app.volumes.confirm = confirm;
            let _ = app.view();
        }
        app.push_toast("x".into(), None);
        let _ = app.update(Message::ToggleMaximize);
        let _ = app.view();
    }

    #[tokio::test]
    async fn navigating_away_clears_the_volume_hover() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = volumes_app(&fake).await;
        let _ = app.update(vol_msg(volumes::Msg::Hover("cache".into())));
        assert_eq!(app.volumes.hovered.as_deref(), Some("cache"));
        let _ = app.update(vol_msg(volumes::Msg::Leave("other".into())));
        assert_eq!(app.volumes.hovered.as_deref(), Some("cache"), "stale exit");
        let _ = app.update(Message::Navigate(Screen::Containers));
        assert_eq!(app.volumes.hovered, None);
    }

    // ---- Networks screen (task 4.2) ----

    fn network_event(action: &str, id: &str) -> DockerEvent {
        DockerEvent {
            kind: ResourceKind::Network,
            action: action.into(),
            id: id.into(),
        }
    }

    fn network_list_calls(fake: &FakeClient) -> usize {
        fake.calls()
            .iter()
            .filter(|c| *c == "list_networks")
            .count()
    }

    fn net_msg(m: networks::Msg) -> Message {
        Message::Networks(m)
    }

    /// Connected, on the Networks screen: `bridge`, `host`, `none` (protected) and `app_net`
    /// (with `web` and `db` connected). By name: `app_net`, `bridge`, `host`, `none`.
    async fn networks_app(fake: &Arc<FakeClient>) -> App {
        let mut app = loaded_app(fake).await;
        let _ = app.run_action(ActionId::GoNetworks);
        assert_eq!(app.screen, Screen::Networks);
        app
    }

    fn network_names(app: &App) -> Vec<&str> {
        app.networks.items.iter().map(|n| n.name.as_str()).collect()
    }

    /// The sample daemon plus an unused `scratch` network, already listed by the app.
    async fn networks_app_with_scratch(fake: &Arc<FakeClient>) -> App {
        let mut app = networks_app(fake).await;
        fake.add_network("scratch", "bridge", Some("172.30.0.0/16"), &[]);
        settle(&mut app, Message::ReloadNetworks).await;
        assert!(network_names(&app).contains(&"scratch"));
        app
    }

    #[tokio::test]
    async fn connecting_loads_the_networks() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let app = loaded_app(&fake).await;
        assert_eq!(app.networks.items.len(), 4);
        assert_eq!(network_list_calls(&fake), 1);
    }

    #[tokio::test]
    async fn ask_remove_on_protected_is_ignored() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        for id in ["bridge-id", "host-id", "none-id"] {
            let _ = app.update(net_msg(networks::Msg::AskRemove(id.into())));
            assert_eq!(app.networks.confirm_remove, None, "{id}");
            // Not even the keyboard offers it.
            app.networks.selected = Some(id.into());
            assert!(!actions::available(&app, ActionId::RemoveSelected), "{id}");
            settle(&mut app, key(iced::keyboard::key::Named::Delete, false)).await;
            assert_eq!(app.networks.confirm_remove, None, "{id}");
        }
        // A confirmation forced open still removes nothing.
        let _ = app.update(net_msg(networks::Msg::Confirm));
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_network")));
        // Nor an id nobody knows.
        let _ = app.update(net_msg(networks::Msg::AskRemove("gone-id".into())));
        assert_eq!(app.networks.confirm_remove, None);
    }

    #[tokio::test]
    async fn confirm_never_removes_a_protected_network() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        app.networks.confirm_remove = Some("bridge-id".into());
        settle(&mut app, net_msg(networks::Msg::Confirm)).await;
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_network")));
        assert_eq!(app.networks.confirm_remove, None);
    }

    #[tokio::test]
    async fn unused_network_is_removed_after_confirming() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app_with_scratch(&fake).await;
        let _ = app.update(net_msg(networks::Msg::AskRemove("scratch-id".into())));
        assert_eq!(app.networks.confirm_remove.as_deref(), Some("scratch-id"));
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_network")));
        settle(&mut app, net_msg(networks::Msg::Confirm)).await;
        assert!(
            fake.calls()
                .contains(&"remove_network:scratch-id".to_string())
        );
        assert_eq!(app.networks.confirm_remove, None);
        assert!(app.networks.pending.is_empty());
        assert!(app.toasts.is_empty());
        // The daemon's `destroy` event refreshes the list.
        settle(
            &mut app,
            Message::Docker(network_event("destroy", "scratch-id")),
        )
        .await;
        assert!(!network_names(&app).contains(&"scratch"));
    }

    #[tokio::test]
    async fn removing_a_network_in_use_asks_and_docker_refuses() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        // The plan allows trying: the dialog warns, Docker has the last word.
        let _ = app.update(net_msg(networks::Msg::AskRemove("app_net-id".into())));
        assert_eq!(app.networks.confirm_remove.as_deref(), Some("app_net-id"));
        let _ = app.view();
        settle(&mut app, net_msg(networks::Msg::Confirm)).await;
        assert!(network_names(&app).contains(&"app_net"), "still listed");
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(
            app.toasts[0].text,
            friendly_error(&DockerError::Conflict(String::new()))
        );
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn network_remove_conflict_keeps_item_and_toasts() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app_with_scratch(&fake).await;
        let _ = app.update(net_msg(networks::Msg::AskRemove("scratch-id".into())));
        fake.fail_next(DockerError::Conflict("network has active endpoints".into()));
        settle(&mut app, net_msg(networks::Msg::Confirm)).await;
        assert!(network_names(&app).contains(&"scratch"));
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(
            app.toasts[0].text,
            friendly_error(&DockerError::Conflict(String::new()))
        );
        assert!(app.networks.pending.is_empty(), "the row is usable again");
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn removing_a_network_twice_runs_one_call() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app_with_scratch(&fake).await;
        let _ = app.update(net_msg(networks::Msg::AskRemove("scratch-id".into())));
        let first = app.update(net_msg(networks::Msg::Confirm));
        let _ = app.update(net_msg(networks::Msg::AskRemove("scratch-id".into())));
        assert_eq!(app.networks.confirm_remove, None, "busy rows are not asked");
        let _ = run(first).await;
        assert_eq!(
            fake.calls()
                .iter()
                .filter(|c| *c == "remove_network:scratch-id")
                .count(),
            1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn network_event_triggers_reload() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = network_list_calls(&fake);
        fake.add_network("scratch", "bridge", None, &[]);
        settle(
            &mut app,
            Message::Docker(network_event("create", "scratch-id")),
        )
        .await;
        assert_eq!(network_list_calls(&fake), before + 1);
        assert_eq!(app.networks.items.len(), 5);
        assert_eq!(list_calls(&fake), 1, "containers are not reloaded for it");
        assert_eq!(volume_list_calls(&fake), 1, "nor are volumes");
    }

    #[tokio::test(start_paused = true)]
    async fn burst_of_network_events_reloads_once() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let before = network_list_calls(&fake);
        let tasks: Vec<_> = (0..5)
            .map(|_| app.update(Message::Docker(network_event("connect", "app_net-id"))))
            .collect();
        let mut produced = Vec::new();
        for task in tasks {
            produced.extend(run(task).await);
        }
        assert_eq!(produced.len(), 1, "only the first event schedules a reload");
        for m in produced {
            settle(&mut app, m).await;
        }
        assert_eq!(network_list_calls(&fake), before + 1);
        // The window is over: the next event schedules again.
        settle(
            &mut app,
            Message::Docker(network_event("disconnect", "app_net-id")),
        )
        .await;
        assert_eq!(network_list_calls(&fake), before + 2);
    }

    #[tokio::test(start_paused = true)]
    async fn container_create_and_destroy_refresh_the_connected_containers() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        for action in ["create", "destroy"] {
            let before = network_list_calls(&fake);
            settle(&mut app, Message::Docker(container_event(action))).await;
            assert_eq!(network_list_calls(&fake), before + 1, "{action}");
        }
        // A stop does not change what is attached: Docker says `disconnect` for that.
        let before = network_list_calls(&fake);
        settle(&mut app, Message::Docker(container_event("die"))).await;
        assert_eq!(network_list_calls(&fake), before);
    }

    #[tokio::test(start_paused = true)]
    async fn network_reload_is_skipped_when_docker_is_unreachable() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        let _ = app.update(Message::Docker(network_event("create", "x")));
        app.conn = Connection::Failed(DockerError::NotRunning);
        let before = network_list_calls(&fake);
        assert!(run(app.update(Message::ReloadNetworks)).await.is_empty());
        assert_eq!(network_list_calls(&fake), before);
        // The flag was cleared: a later event can schedule again.
        app.conn = Connection::Ok;
        let produced = run(app.update(Message::Docker(network_event("create", "y")))).await;
        assert_eq!(produced.len(), 1);
    }

    #[tokio::test]
    async fn an_older_network_load_cannot_overwrite_a_newer_one() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        let stale = app.networks_load_seq;
        settle(&mut app, Message::ReloadNetworks).await;
        assert!(app.networks_load_seq > stale);
        let _ = app.update(net_msg(networks::Msg::Loaded(stale, Ok(vec![]))));
        assert_eq!(app.networks.items.len(), 4);
        let _ = app.update(net_msg(networks::Msg::Loaded(
            stale,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.conn, Connection::Ok);
    }

    #[tokio::test]
    async fn network_load_connection_error_goes_to_onboarding() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        app.networks.confirm_remove = Some("app_net-id".into());
        let seq = app.networks_load_seq;
        let _ = app.update(net_msg(networks::Msg::Loaded(
            seq,
            Err(DockerError::NotRunning),
        )));
        assert_eq!(app.conn, Connection::Failed(DockerError::NotRunning));
        assert_eq!(app.networks.confirm_remove, None);
        let _ = app.view();
    }

    #[tokio::test]
    async fn network_keyboard_selection_moves_in_the_visible_list() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        assert_eq!(app.networks.selected, None);
        let _ = app.update(key_msg("j", false));
        assert_eq!(app.networks.selected.as_deref(), Some("app_net-id"));
        let _ = app.update(named_key(iced::keyboard::key::Named::ArrowDown));
        assert_eq!(app.networks.selected.as_deref(), Some("bridge-id"));
        let _ = app.update(key_msg("k", false));
        assert_eq!(app.networks.selected.as_deref(), Some("app_net-id"));
        let _ = app.update(key_msg("k", false));
        assert_eq!(
            app.networks.selected.as_deref(),
            Some("app_net-id"),
            "no wrap"
        );
        assert_eq!(app.containers.selected, None, "containers are untouched");
        assert_eq!(app.volumes.selected, None, "volumes are untouched");
        app.networks.selected = None;
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(app.networks.selected.as_deref(), Some("none-id"));
    }

    #[tokio::test]
    async fn network_selection_scrolls_the_row_into_view() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        let row = crate::ui::tokens::ROW_HEIGHT;
        app.networks.selected = Some("app_net-id".into());
        let _ = app.update(net_msg(networks::Msg::Scrolled {
            offset: 0.0,
            height: row,
        }));
        let _ = app.run_action(ActionId::SelectNext);
        assert_eq!(app.networks.scroll_y, row, "bridge is the second row");
        let _ = app.run_action(ActionId::SelectPrev);
        assert_eq!(app.networks.scroll_y, 0.0);
    }

    #[tokio::test]
    async fn delete_enter_and_escape_drive_the_network_dialog() {
        use iced::keyboard::key::Named;
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app_with_scratch(&fake).await;
        app.networks.selected = Some("scratch-id".into());
        settle(&mut app, key(Named::Delete, false)).await;
        assert_eq!(app.networks.confirm_remove.as_deref(), Some("scratch-id"));
        let _ = app.view();

        // Esc cancels and removes nothing.
        settle(&mut app, key(Named::Escape, false)).await;
        assert_eq!(app.networks.confirm_remove, None);
        assert!(!fake.calls().iter().any(|c| c.starts_with("remove_network")));

        // Behind the dialog the keys do nothing but Enter and Esc.
        settle(&mut app, key(Named::Delete, false)).await;
        let _ = app.update(key_msg("j", false));
        assert_eq!(app.networks.selected.as_deref(), Some("scratch-id"));
        assert!(!actions::available(&app, ActionId::SelectNext));

        // Enter confirms, even if a text input holds the event.
        settle(&mut app, enter_key(true)).await;
        assert_eq!(app.networks.confirm_remove, None);
        assert!(
            fake.calls()
                .contains(&"remove_network:scratch-id".to_string())
        );
    }

    #[tokio::test]
    async fn enter_on_networks_does_not_open_the_dock() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.networks.selected = Some("app_net-id".into());
        for id in [
            ActionId::OpenSelected,
            ActionId::StartStopSelected,
            ActionId::RestartSelected,
        ] {
            assert!(!actions::available(&app, id), "{id:?}");
        }
        settle(&mut app, enter_key(false)).await;
        assert!(app.dock.is_none());
        assert!(!fake.calls().iter().any(|c| c.starts_with("start")));
    }

    #[tokio::test]
    async fn escape_on_networks_closes_layers_in_order() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        app.networks.query = "app".into();
        app.volumes.query = "ca".into();
        app.containers.query = "we".into();
        app.networks.selected = Some("app_net-id".into());
        app.networks.confirm_remove = Some("app_net-id".into());
        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.networks.confirm_remove, None, "the dialog goes first");
        assert_eq!(app.networks.query, "app");
        let _ = app.run_action(ActionId::Escape);
        assert_eq!(app.networks.query, "", "then the search");
        assert_eq!(app.volumes.query, "ca", "other searches stay");
        assert_eq!(app.containers.query, "we", "other searches stay");
    }

    #[tokio::test]
    async fn search_filters_networks_and_keeps_the_selection_visible() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        app.networks.selected = Some("none-id".into());
        let _ = app.update(net_msg(networks::Msg::Query("host".into())));
        assert_eq!(app.networks.selected.as_deref(), Some("host-id"));
        let _ = app.view();
    }

    #[tokio::test]
    async fn network_search_is_focusable_only_on_the_networks_screen() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        assert!(actions::available(&app, ActionId::FocusSearch));
        let _ = app.run_action(ActionId::FocusSearch);
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert!(!actions::available(&app, ActionId::FocusSearch));
    }

    #[tokio::test]
    async fn remove_selected_on_networks_opens_the_network_dialog_only() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        app.containers.selected = Some("web-id".into());
        app.volumes.selected = Some("cache".into());
        app.networks.selected = Some("app_net-id".into());
        settle(&mut app, Message::Action(ActionId::RemoveSelected)).await;
        assert_eq!(app.containers.confirm_remove, None);
        assert_eq!(app.volumes.confirm, None);
        assert_eq!(app.networks.confirm_remove.as_deref(), Some("app_net-id"));
    }

    #[tokio::test]
    async fn a_dialog_of_another_screen_blocks_remove_selected_on_networks() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app_with_scratch(&fake).await;
        app.networks.selected = Some("scratch-id".into());
        app.volumes.confirm = Some(volumes::VolumeConfirm::Prune(vec![]));
        assert!(!actions::available(&app, ActionId::RemoveSelected));
        assert!(!actions::available(&app, ActionId::SelectNext));
    }

    #[tokio::test]
    async fn sidebar_counts_the_networks_while_connected() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = loaded_app(&fake).await;
        assert_eq!(app.network_count(), Some(4));
        app.conn = Connection::Failed(DockerError::NotRunning);
        assert_eq!(app.network_count(), None);
        app.conn = Connection::Checking;
        assert_eq!(app.network_count(), None);
        let _ = app.view();
    }

    #[tokio::test]
    async fn networks_view_builds_with_dock_dialog_and_toasts() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        let _ = app.update(Message::OpenDock("web-id".into()));
        app.networks.selected = Some("app_net-id".into());
        app.networks.hovered = Some("bridge-id".into());
        app.networks.pending.insert("app_net-id".into());
        for confirm in [
            None,
            Some("app_net-id".to_string()),
            Some("bridge-id".into()),
        ] {
            app.networks.confirm_remove = confirm;
            let _ = app.view();
        }
        app.push_toast("x".into(), None);
        let _ = app.update(Message::ToggleMaximize);
        let _ = app.view();
    }

    #[tokio::test]
    async fn navigating_away_clears_the_network_hover() {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = networks_app(&fake).await;
        let _ = app.update(net_msg(networks::Msg::Hover("bridge-id".into())));
        assert_eq!(app.networks.hovered.as_deref(), Some("bridge-id"));
        let _ = app.update(net_msg(networks::Msg::Leave("other".into())));
        assert_eq!(
            app.networks.hovered.as_deref(),
            Some("bridge-id"),
            "stale exit"
        );
        let _ = app.update(Message::Navigate(Screen::Containers));
        assert_eq!(app.networks.hovered, None);
    }
}
