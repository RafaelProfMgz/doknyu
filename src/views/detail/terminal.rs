//! The Terminal tab: a shell in the container, drawn by `iced_term`.
//!
//! `iced_term` 0.8 can only run a local program in a PTY (see `docs/decisions/0001-terminal.md`),
//! so the session is `docker exec -it ... sh -c '...'` there. The rules that keep it from hanging
//! the app, all of them in `App`:
//!
//! * the subscription of the terminal lives exactly as long as `TerminalState::session` does,
//!   whichever tab is on screen (a paused subscription fills the event channel of the PTY thread,
//!   which then blocks holding the lock of the emulator, and the next draw hangs);
//! * the session and the subscription end in the same `update` (leaving the dock, another
//!   container, the connection failing, the shell exiting): a subscription without a `Terminal`
//!   spins and one whose channel closes outside `Exit` panics;
//! * every session has a new `id` (it is what names its subscription).

use std::io;

use iced::widget::{button, column, container, text};
use iced::{Alignment, Color, Element, Length, Padding, Subscription};
pub use iced_term::TerminalView;
use iced_term::actions::Action;
use iced_term::settings::{BackendSettings, FontSettings, Settings, ThemeSettings};
use iced_term::{BackendCommand, ColorPalette, Command, Terminal};
use rust_i18n::t;

use crate::actions::ActionId;
use crate::app::Message;
use crate::terminal_launcher::exec_args;
use crate::ui::fonts::MONO;
use crate::ui::style;
use crate::ui::tokens::{
    Palette, SPACE_2, SPACE_3, SPACE_4, TERM_BRIGHT_MIX, TERM_DIM_MIX, TERM_LINE_SCALE, TEXT,
};

/// How `docker exec` starts: the CLI is the program of the PTY, so the terminal inside the
/// container sees the size of the PTY. `TERM` is set here and in the arguments (the CLI does not
/// pass its own on), as an app started from a launcher has none.
pub fn backend_settings(container_id: &str) -> BackendSettings {
    BackendSettings {
        program: "docker".to_owned(),
        args: exec_args(container_id),
        env: [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        working_directory: None,
    }
}

/// `#rrggbb`, the only form `iced_term` reads (it panics on anything else).
fn hex(c: Color) -> String {
    let [r, g, b, _] = c.into_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `a` moved `t` (0..=1) of the way to `b`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}

/// The 16 ANSI colors from the app palette, so the terminal follows the theme. The emulator has
/// no color of its own for the cursor (it draws it in the text color), so there is no accent.
/// There is no magenta or cyan in the palette: they are blends of its neighbors.
pub fn color_palette(p: &Palette) -> ColorPalette {
    let bright = |c: Color| hex(mix(c, p.text, TERM_BRIGHT_MIX));
    let dim = |c: Color| hex(mix(c, p.bg, TERM_DIM_MIX));
    let magenta = mix(p.accent, p.danger, 0.5);
    let cyan = mix(p.accent, p.success, 0.5);
    ColorPalette {
        foreground: hex(p.text),
        background: hex(p.bg),
        black: hex(p.border),
        red: hex(p.danger),
        green: hex(p.success),
        yellow: hex(p.warning),
        blue: hex(p.accent),
        magenta: hex(magenta),
        cyan: hex(cyan),
        white: hex(p.muted),
        bright_black: hex(p.faint),
        bright_red: bright(p.danger),
        bright_green: bright(p.success),
        bright_yellow: bright(p.warning),
        bright_blue: bright(p.accent),
        bright_magenta: bright(magenta),
        bright_cyan: bright(cyan),
        bright_white: hex(p.text),
        bright_foreground: None,
        dim_foreground: dim(p.text),
        dim_black: dim(p.border),
        dim_red: dim(p.danger),
        dim_green: dim(p.success),
        dim_yellow: dim(p.warning),
        dim_blue: dim(p.accent),
        dim_magenta: dim(magenta),
        dim_cyan: dim(cyan),
        dim_white: dim(p.muted),
    }
}

/// Everything `Terminal::new` needs for a session in `container_id`.
pub fn settings(container_id: &str, p: &Palette) -> Settings {
    Settings {
        font: FontSettings {
            size: TEXT,
            scale_factor: TERM_LINE_SCALE,
            font_type: MONO,
        },
        theme: ThemeSettings::new(Box::new(color_palette(p))),
        backend: backend_settings(container_id),
    }
}

/// The terminal session of the dock. It belongs to the container the dock shows and is dropped
/// with it: outside of tests `active_id.is_some() == session.is_some()`.
#[derive(Default)]
pub struct TerminalState {
    pub session: Option<Terminal>,
    /// Id of the live session, the one its events carry; anything else is from a session that
    /// is gone.
    pub active_id: Option<u64>,
    /// Id of the next session; never reused, as it names the subscription.
    pub next_id: u64,
    /// The last session ended (the shell exited, the container stopped, or it did not start).
    /// No new one starts by itself until the user asks, or the container changes.
    pub ended: bool,
}

impl TerminalState {
    /// Drops the session (which kills the `docker exec` and its PTY) and its id together.
    pub fn close(&mut self) {
        self.session = None;
        self.active_id = None;
    }

    /// Like [`Self::close`], for a dock that goes away or shows another container: also forgets
    /// that a session ended.
    pub fn reset(&mut self) {
        self.close();
        self.ended = false;
    }

    /// The shell ended: no session, and the tab says so.
    pub fn shutdown(&mut self) {
        self.close();
        self.ended = true;
    }

    /// Starts a session with the next id. `create` is given the id; when it fails nothing is
    /// left active, and the id is not used again.
    pub fn start(&mut self, create: impl FnOnce(u64) -> io::Result<Terminal>) -> io::Result<()> {
        let id = self.next_id;
        self.next_id += 1;
        let session = create(id)?;
        self.session = Some(session);
        self.active_id = Some(id);
        self.ended = false;
        Ok(())
    }

    /// Hands an event of the session `id` to the emulator and returns what it asks of the app.
    /// An event of any other id is from a session that was dropped and does nothing.
    pub fn handle(&mut self, id: u64, cmd: BackendCommand) -> Action {
        if self.active_id != Some(id) {
            return Action::Ignore;
        }
        match &mut self.session {
            Some(session) => session.handle(Command::ProxyToBackend(cmd)),
            None => Action::Ignore,
        }
    }

    /// Paints the session in the colors of `p`.
    pub fn set_palette(&mut self, p: &Palette) {
        if let Some(session) = &mut self.session {
            let _ = session.handle(Command::ChangeTheme(Box::new(color_palette(p))));
        }
    }
}

/// The events of the live session; none without one.
pub fn subscription(state: &TerminalState) -> Subscription<Message> {
    state
        .session
        .as_ref()
        .map_or_else(Subscription::none, |session| {
            session.subscription().map(Message::Terminal)
        })
}

/// What the tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// The terminal itself.
    Session,
    NotRunning,
    NoCli,
    Ended,
}

/// Picks what the tab shows: a live session always wins; without one, no `docker` CLI is the
/// news, then a stopped container; a running one with no session means the last one ended.
pub fn content(session: bool, running: bool, docker_cli: bool) -> Content {
    if session {
        Content::Session
    } else if !docker_cli {
        Content::NoCli
    } else if !running {
        Content::NotRunning
    } else {
        Content::Ended
    }
}

fn notice<'a>(lines: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(column(lines).spacing(SPACE_3).align_x(Alignment::Center))
        .center(Length::Fill)
        .padding(SPACE_4)
        .style(style::container::bg)
        .into()
}

fn hint<'a>(key: &str) -> Element<'a, Message> {
    text(t!(key).into_owned())
        .size(TEXT)
        .style(style::text::muted)
        .into()
}

fn action<'a>(label: String, on_press: Message) -> Element<'a, Message> {
    button(text(label).size(TEXT))
        .padding(Padding::from([SPACE_2, SPACE_3]))
        .style(style::button::ghost)
        .on_press(on_press)
        .into()
}

/// The body of the Terminal tab.
pub fn view<'a>(state: &'a TerminalState, running: bool, docker_cli: bool) -> Element<'a, Message> {
    match (
        content(state.session.is_some(), running, docker_cli),
        &state.session,
    ) {
        (Content::Session, Some(session)) => TerminalView::show(session).map(Message::Terminal),
        (Content::NotRunning, _) => notice(vec![hint("terminal.not_running")]),
        (Content::NoCli, _) => {
            let mut lines = vec![hint("terminal.no_cli")];
            if running {
                lines.push(action(
                    t!("dock.open_terminal").into_owned(),
                    Message::Action(ActionId::OpenExternalTerminal),
                ));
            }
            notice(lines)
        }
        // `Session` without a session cannot happen; if it ever did, offering a new one is right.
        (Content::Ended, _) | (Content::Session, None) => notice(vec![
            hint("terminal.ended"),
            action(
                t!("terminal.new_session").into_owned(),
                Message::TerminalNewSession,
            ),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tokens::{DARK, LIGHT, Palette};

    #[test]
    fn backend_settings_runs_docker_exec() {
        let settings = backend_settings("web-id");
        assert_eq!(settings.program, "docker");
        assert_eq!(
            settings.args[..7],
            [
                "exec",
                "-it",
                "-e",
                "TERM=xterm-256color",
                "web-id",
                "sh",
                "-c"
            ]
        );
        assert_eq!(settings.args, crate::terminal_launcher::exec_args("web-id"));
        assert_eq!(settings.env["TERM"], "xterm-256color");
        assert_eq!(settings.env["COLORTERM"], "truecolor");
    }

    fn fields(c: &iced_term::ColorPalette) -> Vec<&str> {
        let mut all = vec![
            &c.foreground,
            &c.background,
            &c.black,
            &c.red,
            &c.green,
            &c.yellow,
            &c.blue,
            &c.magenta,
            &c.cyan,
            &c.white,
            &c.bright_black,
            &c.bright_red,
            &c.bright_green,
            &c.bright_yellow,
            &c.bright_blue,
            &c.bright_magenta,
            &c.bright_cyan,
            &c.bright_white,
            &c.dim_foreground,
            &c.dim_black,
            &c.dim_red,
            &c.dim_green,
            &c.dim_yellow,
            &c.dim_blue,
            &c.dim_magenta,
            &c.dim_cyan,
            &c.dim_white,
        ];
        all.extend(c.bright_foreground.iter());
        all.into_iter().map(String::as_str).collect()
    }

    #[test]
    fn colors_are_the_seven_characters_iced_term_parses() {
        // `iced_term` panics on anything but `#rrggbb`.
        for p in [&DARK, &LIGHT] {
            for color in fields(&color_palette(p)) {
                assert_eq!(color.len(), 7, "{color}");
                assert!(color.starts_with('#'), "{color}");
                assert!(u32::from_str_radix(&color[1..], 16).is_ok(), "{color}");
            }
        }
    }

    #[test]
    fn colors_follow_the_active_palette() {
        for p in [&DARK, &LIGHT] {
            let c = color_palette(p);
            assert_eq!(c.background, hex(p.bg));
            assert_eq!(c.foreground, hex(p.text));
            assert_eq!(c.red, hex(p.danger));
            assert_eq!(c.green, hex(p.success));
            assert_eq!(c.yellow, hex(p.warning));
            assert_eq!(c.blue, hex(p.accent));
        }
        assert_ne!(
            color_palette(&DARK).background,
            color_palette(&LIGHT).background
        );
    }

    #[test]
    fn hex_writes_rrggbb() {
        let p = Palette {
            bg: iced::Color::from_rgb8(0x28, 0x2c, 0x33),
            ..DARK
        };
        assert_eq!(hex(p.bg), "#282c33");
    }

    #[test]
    fn settings_use_the_mono_font_and_the_container() {
        let s = settings("web-id", &DARK);
        assert_eq!(s.font.font_type, crate::ui::fonts::MONO);
        assert_eq!(s.backend.args[4], "web-id");
        assert_eq!(s.theme.color_pallete.background, hex(DARK.bg));
    }

    #[test]
    fn close_clears_session_and_id_together() {
        let mut state = TerminalState {
            active_id: Some(1),
            ..TerminalState::default()
        };
        state.close();
        assert_eq!(state.active_id, None);
        assert!(state.session.is_none());
    }

    #[test]
    fn close_keeps_ended_but_reset_clears_it() {
        let mut state = TerminalState {
            active_id: Some(1),
            ended: true,
            ..TerminalState::default()
        };
        state.close();
        assert!(state.ended);
        state.reset();
        assert!(!state.ended);
    }

    #[test]
    fn shutdown_marks_ended() {
        let mut state = TerminalState {
            active_id: Some(4),
            next_id: 5,
            ..TerminalState::default()
        };
        state.shutdown();
        assert!(state.ended);
        assert_eq!(state.active_id, None);
        assert!(state.session.is_none());
        // The next session gets a new id: the id names the subscription.
        assert_eq!(state.next_id, 5);
    }

    #[test]
    fn a_failed_start_burns_its_id_and_leaves_nothing_active() {
        let mut state = TerminalState::default();
        let mut seen = Vec::new();
        for _ in 0..2 {
            let result = state.start(|id| {
                seen.push(id);
                Err(std::io::Error::other("no pty"))
            });
            assert!(result.is_err());
            assert_eq!(state.active_id, None);
            assert!(state.session.is_none());
        }
        assert_eq!(seen, [0, 1]);
    }

    #[test]
    fn events_of_another_session_are_dropped() {
        let mut state = TerminalState {
            active_id: Some(2),
            ..TerminalState::default()
        };
        let exit =
            iced_term::BackendCommand::ProcessAlacrittyEvent(iced_term::AlacrittyEvent::Exit);
        // Id 1 is a session that is gone; even `Exit` must not end the one of id 2.
        assert_eq!(
            state.handle(1, exit.clone()),
            iced_term::actions::Action::Ignore
        );
        // No session behind the id (as in these tests): nothing to ask.
        assert_eq!(state.handle(2, exit), iced_term::actions::Action::Ignore);
    }

    #[test]
    fn stopped_container_shows_hint() {
        assert_eq!(content(false, false, true), Content::NotRunning);
    }

    #[test]
    fn docker_cli_missing_shows_hint() {
        assert_eq!(content(false, true, false), Content::NoCli);
        assert_eq!(content(false, false, false), Content::NoCli);
    }

    #[test]
    fn running_container_without_session_offers_a_new_one() {
        assert_eq!(content(false, true, true), Content::Ended);
    }

    #[test]
    fn a_live_session_is_always_shown() {
        for (running, docker_cli) in [(true, true), (false, true), (false, false)] {
            assert_eq!(content(true, running, docker_cli), Content::Session);
        }
    }

    #[test]
    fn texts_are_translated() {
        for key in [
            "terminal.not_running",
            "terminal.no_cli",
            "terminal.ended",
            "terminal.new_session",
            "terminal.start_failed",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(rust_i18n::t!(key, locale = locale), key, "{locale} {key}");
            }
        }
    }

    #[test]
    fn view_builds_for_every_state_without_a_session() {
        for (running, docker_cli, ended) in [
            (false, true, false),
            (true, false, false),
            (false, false, false),
            (true, true, true),
        ] {
            let state = TerminalState {
                ended,
                ..TerminalState::default()
            };
            let _: iced::Element<'_, crate::app::Message> = view(&state, running, docker_cli);
        }
    }
}
