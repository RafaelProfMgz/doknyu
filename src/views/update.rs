//! The "Check for updates" dialog. The work is in `crate::update`; `App` runs it off the UI
//! thread and feeds the download progress back here.

use std::path::PathBuf;

use iced::widget::{button, column, container, progress_bar, row, scrollable, space, text};
use iced::{Element, Length};
use rust_i18n::t;

use crate::app::Message;
use crate::install::InstallKind;
use crate::ui::fonts::SANS_SEMIBOLD;
use crate::ui::style;
use crate::ui::tokens::{SPACE_2, SPACE_4, TEXT_LG, TEXT_SM};
use crate::update::{CURRENT, Release, UpdateError, UpdatePlan};
use crate::views::widgets::command_block;

/// Height of the release notes before they scroll (about 12 lines).
const NOTES_HEIGHT: f32 = 12.0 * 18.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Finding out how the app was installed.
    Detecting,
    /// Asking GitHub for the newest release.
    Checking,
    UpToDate,
    /// A newer release exists: "Update and restart" (or "Open release page").
    Available(Release),
    /// The package manager decides (dnf): "Update" before asking for the password.
    ConfirmPrivileged,
    Downloading {
        done: u64,
        total: u64,
    },
    /// The download is done (or not needed) and the package manager is working.
    Installing,
    /// Only a command to show.
    Command(String),
    Error {
        message: String,
        /// A page where the user can get the new version by hand.
        release_url: Option<String>,
    },
    /// The update worked but the new version could not be started.
    RestartFailed,
}

/// How an update ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The new version is running; this one closes.
    Restarted,
    /// Updated, but the new version did not start: the user opens it.
    RestartFailed,
    /// The package manager found nothing newer.
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateState {
    pub kind: Option<InstallKind>,
    pub plan: Option<UpdatePlan>,
    /// The executable to start after the update (`.deb` and dnf keep the same path).
    pub exe: Option<PathBuf>,
    pub stage: Stage,
    /// The release being installed, to go back to after a cancel and to link on errors.
    pub release: Option<Release>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            kind: None,
            plan: None,
            exe: None,
            stage: Stage::Detecting,
            release: None,
        }
    }
}

impl UpdateState {
    /// Whether the dialog may close now: never in the middle of a download or install.
    pub fn can_close(&self) -> bool {
        !matches!(self.stage, Stage::Downloading { .. } | Stage::Installing)
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Detected {
        kind: InstallKind,
        plan: UpdatePlan,
        exe: Option<PathBuf>,
    },
    Checked(Result<Release, UpdateError>),
    /// "Update and restart" / "Update" / "Open release page".
    Start,
    Progress(u64, u64),
    Finished(Result<Outcome, UpdateError>),
    Retry,
    Close,
    /// Closes the app after "Updated, open doknyu again".
    Quit,
}

fn msg(m: Msg) -> Message {
    Message::Update(m)
}

/// Plain-language text for a failed update.
pub fn error_text(e: &UpdateError) -> String {
    match e {
        UpdateError::Network => t!("update.error.network").into_owned(),
        UpdateError::RateLimited => t!("update.error.rate_limited").into_owned(),
        UpdateError::Http(code) => t!("update.error.http", code = code).into_owned(),
        UpdateError::BadRelease => t!("update.error.bad_release").into_owned(),
        UpdateError::MissingAsset => t!("update.error.missing_asset").into_owned(),
        UpdateError::HashMismatch => t!("update.error.hash").into_owned(),
        UpdateError::NotWritable(dir) => {
            t!("update.error.not_writable", dir = dir.display().to_string()).into_owned()
        }
        UpdateError::Cancelled => t!("update.cancelled").into_owned(),
        UpdateError::Failed(tail) => format!("{}\n{tail}", t!("update.error.failed")),
        UpdateError::Io(e) => format!("{}\n{e}", t!("update.error.io")),
    }
}

fn ghost(label: String, on_press: Option<Message>) -> Element<'static, Message> {
    button(text(label))
        .style(style::button::ghost)
        .on_press_maybe(on_press)
        .into()
}

fn primary(label: String, on_press: Option<Message>) -> Element<'static, Message> {
    button(text(label))
        .style(style::button::primary)
        .on_press_maybe(on_press)
        .into()
}

pub fn dialog(state: &UpdateState) -> Element<'static, Message> {
    let close = || ghost(t!("common.close").into_owned(), Some(msg(Msg::Close)));
    let mut card = column![
        text(t!("update.title").into_owned())
            .size(TEXT_LG)
            .font(SANS_SEMIBOLD)
    ]
    .spacing(SPACE_4);
    let footer: Vec<Element<'static, Message>> = match &state.stage {
        Stage::Detecting | Stage::Checking => {
            card = card.push(text(t!("update.checking").into_owned()));
            vec![close()]
        }
        Stage::UpToDate => {
            card = card.push(text(
                t!("update.up_to_date", version = CURRENT).into_owned(),
            ));
            vec![close()]
        }
        Stage::Available(release) => {
            card = card.push(
                text(format!("{CURRENT} → {}", release.version))
                    .size(TEXT_LG)
                    .font(SANS_SEMIBOLD),
            );
            if !release.notes.trim().is_empty() {
                card = card.push(
                    container(
                        scrollable(text(release.notes.clone()).size(TEXT_SM))
                            .direction(style::thin_scrollbar())
                            .style(style::scrollable_thin),
                    )
                    .max_height(NOTES_HEIGHT)
                    .padding(SPACE_2)
                    .style(style::container::bordered),
                );
            }
            let label = if state.plan == Some(UpdatePlan::OpenRelease) {
                t!("update.open_release")
            } else {
                t!("update.update_and_restart")
            };
            vec![
                ghost(t!("update.not_now").into_owned(), Some(msg(Msg::Close))),
                primary(label.into_owned(), Some(msg(Msg::Start))),
            ]
        }
        Stage::ConfirmPrivileged => {
            card = card.push(text(t!("update.privileged_hint").into_owned()));
            vec![
                ghost(t!("common.cancel").into_owned(), Some(msg(Msg::Close))),
                primary(t!("update.update").into_owned(), Some(msg(Msg::Start))),
            ]
        }
        Stage::Downloading { done, total } => {
            let fraction = if *total == 0 {
                0.0
            } else {
                (*done as f32 / *total as f32).min(1.0)
            };
            card = card
                .push(text(t!("update.downloading").into_owned()))
                .push(progress_bar(0.0..=1.0, fraction).girth(6))
                .push(
                    text(format!(
                        "{} / {}",
                        crate::views::images::format_size(*done),
                        crate::views::images::format_size(*total)
                    ))
                    .size(TEXT_SM)
                    .style(style::text::muted),
                );
            vec![ghost(t!("common.close").into_owned(), None)]
        }
        Stage::Installing => {
            card = card.push(text(t!("update.installing").into_owned()));
            vec![ghost(t!("common.close").into_owned(), None)]
        }
        Stage::Command(command) => {
            card = card
                .push(text(t!("update.run_command").into_owned()))
                .push(command_block(command.clone()));
            vec![close()]
        }
        Stage::Error {
            message,
            release_url,
        } => {
            card = card.push(text(message.clone()).style(style::text::danger));
            let mut buttons = vec![close()];
            if let Some(url) = release_url {
                buttons.push(ghost(
                    t!("update.open_release").into_owned(),
                    Some(Message::OpenUrl(url.clone())),
                ));
            }
            buttons.push(primary(
                t!("update.retry").into_owned(),
                Some(msg(Msg::Retry)),
            ));
            buttons
        }
        Stage::RestartFailed => {
            card = card.push(text(t!("update.restart_failed").into_owned()));
            vec![primary(
                t!("common.close").into_owned(),
                Some(msg(Msg::Quit)),
            )]
        }
    };
    let mut bar = row![space().width(Length::Fill)].spacing(SPACE_2);
    for b in footer {
        bar = bar.push(b);
    }
    card.push(bar).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_builds_in_every_state() {
        let release = Release {
            version: "9.0.0".into(),
            html_url: "https://x".into(),
            notes: "- a\n- b".into(),
            assets: vec![],
        };
        let mut state = UpdateState::default();
        for stage in [
            Stage::Detecting,
            Stage::Checking,
            Stage::UpToDate,
            Stage::Available(release),
            Stage::ConfirmPrivileged,
            Stage::Downloading { done: 5, total: 10 },
            Stage::Downloading { done: 0, total: 0 },
            Stage::Installing,
            Stage::Command("brew upgrade doknyu".into()),
            Stage::Error {
                message: "x".into(),
                release_url: Some("https://x".into()),
            },
            Stage::RestartFailed,
        ] {
            state.stage = stage;
            let _ = dialog(&state);
        }
    }

    #[test]
    fn closes_except_while_working() {
        let mut state = UpdateState::default();
        assert!(state.can_close());
        state.stage = Stage::Downloading { done: 1, total: 2 };
        assert!(!state.can_close());
        state.stage = Stage::Installing;
        assert!(!state.can_close());
        state.stage = Stage::UpToDate;
        assert!(state.can_close());
    }

    #[test]
    fn texts_are_translated() {
        for key in [
            "update.title",
            "update.checking",
            "update.up_to_date",
            "update.open_release",
            "update.update_and_restart",
            "update.not_now",
            "update.privileged_hint",
            "update.update",
            "update.downloading",
            "update.installing",
            "update.run_command",
            "update.retry",
            "update.restart_failed",
            "update.cancelled",
            "update.error.network",
            "update.error.rate_limited",
            "update.error.http",
            "update.error.bad_release",
            "update.error.missing_asset",
            "update.error.hash",
            "update.error.not_writable",
            "update.error.failed",
            "update.error.io",
            "actions.check_update",
            "sidebar.check_update",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(key, locale = locale, version = "1", code = 1, dir = "x"),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }
}
