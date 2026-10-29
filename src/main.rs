use std::sync::Arc;

use doknyu::app::{App, normalize_locale};
use doknyu::docker::DockerClient;
use doknyu::ui::{fonts, prefs};
use tracing_subscriber::EnvFilter;

#[cfg(feature = "fake")]
fn make_client() -> Arc<dyn DockerClient> {
    Arc::new(doknyu::docker::FakeClient::with_sample_data())
}

#[cfg(not(feature = "fake"))]
fn make_client() -> Arc<dyn DockerClient> {
    doknyu::docker::connect()
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let locale = sys_locale::get_locale().unwrap_or_default();
    rust_i18n::set_locale(normalize_locale(&locale));

    let client = make_client();
    let mut app = iced::application(
        move || App::new_with_config(client.clone(), prefs::config_dir()),
        App::update,
        App::view,
    )
    .subscription(App::subscription)
    .theme(App::theme)
    .default_font(fonts::SANS)
    .title("doknyu");
    for bytes in fonts::all_bytes() {
        app = app.font(bytes);
    }
    app.run()
}
