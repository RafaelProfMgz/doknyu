//! The "Uninstall doknyu" dialog: says how the app was installed and removes it, or shows the
//! command that does. The work itself is in `crate::uninstall`; `App` runs it off the UI thread.

use std::path::PathBuf;

use iced::widget::{button, checkbox, column, row, space, text};
use iced::{Element, Length};
use rust_i18n::t;

use crate::app::Message;
use crate::install::InstallKind;
use crate::ui::fonts::{MONO, SANS_SEMIBOLD};
use crate::ui::style;
use crate::ui::tokens::{SPACE_2, SPACE_4, TEXT_LG, TEXT_SM};
use crate::uninstall::{UninstallError, UninstallPlan};
use crate::views::widgets::command_block;

/// The "Uninstalling" section of the README, for installs the app cannot recognize.
pub const README_UNINSTALL: &str = "https://github.com/RafaelProfMgz/doknyu#uninstalling";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Asking the package managers how the app was installed.
    Detecting,
    Ready,
    /// The removal is running: nothing can be clicked and the dialog does not close.
    Running,
    /// The last try failed; the message is shown and the user can try again.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallState {
    pub kind: Option<InstallKind>,
    pub plan: Option<UninstallPlan>,
    /// Also remove the preferences directory.
    pub delete_prefs: bool,
    /// Where the preferences are, shown beside the checkbox.
    pub prefs: Option<PathBuf>,
    pub stage: Stage,
}

impl UninstallState {
    pub fn new(prefs: Option<PathBuf>) -> Self {
        Self {
            kind: None,
            plan: None,
            delete_prefs: true,
            prefs,
            stage: Stage::Detecting,
        }
    }

    /// Whether the dialog may close now.
    pub fn can_close(&self) -> bool {
        self.stage != Stage::Running
    }

    /// The preferences directory to remove along with the app, if the user wants it. Flatpak
    /// removes its own data (`--delete-data`).
    pub fn prefs_to_delete(&self) -> Option<PathBuf> {
        let flatpak = self.kind == Some(InstallKind::Flatpak);
        (self.delete_prefs && !flatpak)
            .then(|| self.prefs.clone())
            .flatten()
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Detected(InstallKind, UninstallPlan),
    DeletePrefs(bool),
    /// "Uninstall", or "Delete preferences and close" when only a command is shown.
    Confirm,
    Cancel,
    Done(Result<(), UninstallError>),
}

fn msg(m: Msg) -> Message {
    Message::Uninstall(m)
}

/// The sentence that says how the app was installed.
pub fn detected_text(kind: &InstallKind) -> String {
    match kind {
        InstallKind::AppImage { path } => {
            t!("uninstall.via_appimage", path = path.display().to_string())
        }
        InstallKind::Deb { package } => t!("uninstall.via_deb", package = package),
        InstallKind::Rpm { package } => t!("uninstall.via_rpm", package = package),
        InstallKind::Pacman { package } => t!("uninstall.via_pacman", package = package),
        InstallKind::Flatpak => t!("uninstall.via_flatpak"),
        InstallKind::Nix => t!("uninstall.via_nix"),
        InstallKind::Homebrew => t!("uninstall.via_homebrew"),
        InstallKind::Cargo => t!("uninstall.via_cargo"),
        InstallKind::Unknown { .. } => t!("uninstall.via_unknown"),
    }
    .into_owned()
}

/// Plain-language text for a failed uninstall.
pub fn error_text(e: &UninstallError) -> String {
    match e {
        UninstallError::Cancelled => t!("uninstall.cancelled").into_owned(),
        UninstallError::Failed(tail) => format!("{}\n{tail}", t!("uninstall.failed")),
        UninstallError::Files(list) => {
            let files: Vec<String> = list
                .iter()
                .map(|(path, why)| format!("{}: {why}", path.display()))
                .collect();
            format!("{}\n{}", t!("uninstall.files_failed"), files.join("\n"))
        }
    }
}

pub fn dialog(state: &UninstallState) -> Element<'static, Message> {
    let running = state.stage == Stage::Running;
    let mut card = column![
        text(t!("uninstall.title").into_owned())
            .size(TEXT_LG)
            .font(SANS_SEMIBOLD)
    ]
    .spacing(SPACE_4);
    let (Some(kind), Some(plan)) = (&state.kind, &state.plan) else {
        card = card.push(text(t!("uninstall.detecting").into_owned()));
        return card
            .push(row![
                space().width(Length::Fill),
                button(text(t!("common.cancel").into_owned()))
                    .style(style::button::ghost)
                    .on_press(msg(Msg::Cancel)),
            ])
            .into();
    };
    card = card
        .push(text(detected_text(kind)))
        .push(text(t!("uninstall.docker_safe").into_owned()).style(style::text::muted));

    let command = match plan {
        UninstallPlan::ShowCommand { command } => Some(command.as_str()),
        _ => None,
    };
    match (command, kind) {
        (Some(""), InstallKind::Unknown { exe }) => {
            card = card.push(
                text(t!("uninstall.unknown_hint", path = exe.display().to_string()).into_owned())
                    .size(TEXT_SM),
            );
            card = card.push(
                button(text(t!("uninstall.open_docs").into_owned()))
                    .style(style::button::link)
                    .on_press(Message::OpenUrl(README_UNINSTALL.to_owned())),
            );
        }
        (Some(command), _) => {
            card = card
                .push(text(t!("uninstall.run_command").into_owned()))
                .push(command_block(command.to_owned()));
        }
        (None, _) => {}
    }

    if *kind != InstallKind::Flatpak
        && let Some(prefs) = &state.prefs
    {
        card = card.push(
            column![
                checkbox(state.delete_prefs)
                    .label(t!("uninstall.delete_prefs").into_owned())
                    .on_toggle_maybe((!running).then_some(|v| msg(Msg::DeletePrefs(v)))),
                text(prefs.display().to_string())
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted),
            ]
            .spacing(SPACE_2),
        );
    }

    if let Stage::Failed(message) = &state.stage {
        card = card.push(
            text(message.clone())
                .size(TEXT_SM)
                .style(style::text::danger),
        );
    }
    if running {
        card = card.push(text(t!("uninstall.running").into_owned()).style(style::text::muted));
    }

    let mut footer = row![space().width(Length::Fill)].spacing(SPACE_2);
    let close_label = if command.is_some() {
        t!("common.close")
    } else {
        t!("common.cancel")
    };
    footer = footer.push(
        button(text(close_label.into_owned()))
            .style(style::button::ghost)
            .on_press_maybe((!running).then(|| msg(Msg::Cancel))),
    );
    let main = match command {
        None => Some(t!("uninstall.confirm")),
        // Only a command to show: the app can still forget the preferences and close.
        Some(_) if state.prefs_to_delete().is_some() => Some(t!("uninstall.delete_prefs_and_quit")),
        Some(_) => None,
    };
    if let Some(label) = main {
        footer = footer.push(
            button(text(label.into_owned()))
                .style(style::button::danger)
                .on_press_maybe((!running).then(|| msg(Msg::Confirm))),
        );
    }
    card.push(footer).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_builds_in_every_state() {
        let kinds = [
            (
                InstallKind::AppImage {
                    path: "/a/doknyu.AppImage".into(),
                },
                UninstallPlan::RemoveFiles {
                    files: vec!["/a/doknyu.AppImage".into()],
                },
            ),
            (
                InstallKind::Deb {
                    package: "doknyu".into(),
                },
                UninstallPlan::Privileged {
                    program: "pkexec",
                    args: vec![],
                },
            ),
            (
                InstallKind::Flatpak,
                UninstallPlan::ShowCommand {
                    command: "flatpak uninstall".into(),
                },
            ),
            (
                InstallKind::Unknown { exe: "/x".into() },
                UninstallPlan::ShowCommand {
                    command: String::new(),
                },
            ),
        ];
        let mut state = UninstallState::new(Some("/home/u/.config/doknyu".into()));
        let _ = dialog(&state);
        for (kind, plan) in kinds {
            for stage in [Stage::Ready, Stage::Running, Stage::Failed("boom".into())] {
                state.kind = Some(kind.clone());
                state.plan = Some(plan.clone());
                state.stage = stage;
                let _ = dialog(&state);
            }
        }
    }

    #[test]
    fn flatpak_never_deletes_prefs_itself() {
        let mut state = UninstallState::new(Some("/p".into()));
        state.kind = Some(InstallKind::Homebrew);
        assert_eq!(state.prefs_to_delete(), Some("/p".into()));
        state.delete_prefs = false;
        assert_eq!(state.prefs_to_delete(), None);
        state.delete_prefs = true;
        state.kind = Some(InstallKind::Flatpak);
        assert_eq!(state.prefs_to_delete(), None);
    }

    #[test]
    fn texts_are_translated() {
        for key in [
            "uninstall.title",
            "uninstall.detecting",
            "uninstall.via_appimage",
            "uninstall.via_deb",
            "uninstall.via_rpm",
            "uninstall.via_pacman",
            "uninstall.via_flatpak",
            "uninstall.via_nix",
            "uninstall.via_homebrew",
            "uninstall.via_cargo",
            "uninstall.via_unknown",
            "uninstall.docker_safe",
            "uninstall.unknown_hint",
            "uninstall.open_docs",
            "uninstall.run_command",
            "uninstall.delete_prefs",
            "uninstall.running",
            "uninstall.confirm",
            "uninstall.delete_prefs_and_quit",
            "uninstall.cancelled",
            "uninstall.failed",
            "uninstall.files_failed",
            "actions.uninstall",
            "sidebar.uninstall",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(key, locale = locale, path = "x", package = "x"),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }
}
