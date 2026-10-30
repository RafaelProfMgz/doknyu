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

/// App ID (também o nome do `.desktop`, do ícone e do metainfo). Vira o `app_id` do Wayland e o
/// `WM_CLASS` do X11, que o `StartupWMClass` do `.desktop` precisa igualar.
const APP_ID: &str = "io.github.rafaelprofmgz.Doknyu";

const ICON_PNG: &[u8] =
    include_bytes!("../assets/icons/hicolor/256x256/apps/io.github.rafaelprofmgz.Doknyu.png");

/// Decodifica o PNG embutido (RGBA 8 bits) para o ícone da janela.
fn window_icon() -> anyhow::Result<iced::window::Icon> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or_default()];
    let info = reader.next_frame(&mut buf)?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight,
        "o ícone embutido deve ser RGBA de 8 bits, veio {:?}/{:?}",
        info.color_type,
        info.bit_depth
    );
    buf.truncate(info.buffer_size());
    Ok(iced::window::icon::from_rgba(buf, info.width, info.height)?)
}

fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: iced::Size::new(1200.0, 760.0),
        min_size: Some(iced::Size::new(900.0, 600.0)),
        // Sem ícone a janela ainda abre; só registra o problema.
        icon: window_icon()
            .inspect_err(|e| tracing::warn!("ícone da janela indisponível: {e}"))
            .ok(),
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: APP_ID.to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn main() -> iced::Result {
    // `doknyu --exec-bridge <container>` is the shell of the terminals: nothing of the app runs.
    let args: Vec<String> = std::env::args().collect();
    match doknyu::exec_bridge::bridge_target(&args) {
        Ok(Some(container)) => std::process::exit(doknyu::exec_bridge::main(&container)),
        Ok(None) => {}
        Err(usage) => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    }

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
    .window(window_settings())
    .title("doknyu");
    for bytes in fonts::all_bytes() {
        app = app.font(bytes);
    }
    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_decodes() {
        let icon = window_icon().expect("o PNG embutido deve virar um window::Icon");
        let (rgba, size) = icon.into_raw();
        assert_eq!((size.width, size.height), (256, 256));
        assert_eq!(rgba.len(), 256 * 256 * 4);
    }

    #[test]
    fn window_settings_match_the_design() {
        let s = window_settings();
        assert_eq!(s.size, iced::Size::new(1200.0, 760.0));
        assert_eq!(s.min_size, Some(iced::Size::new(900.0, 600.0)));
        assert!(s.icon.is_some());
        assert_eq!(s.platform_specific.application_id, APP_ID);
    }
}
