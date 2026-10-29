pub mod actions;
pub mod app;
pub mod docker;
pub mod error;
pub mod keymap;
pub mod model;
pub mod stats;
pub mod ui;
pub mod views;

rust_i18n::i18n!("locales", fallback = "en");
