use anyhow::Result;
use std::path::PathBuf;
use std::sync::OnceLock;
use tracing::{info, warn};

use config::{Config, FileFormat};

pub fn get_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();

    CONFIG.get_or_init(|| build_config().unwrap())
}

/// Get the location of the first found config file paths according to the following order:
///
/// 1. meshstellar.toml
/// 2. $XDG_CONFIG_HOME/meshstellar/meshstellar.toml
/// 3. $HOME/.meshstellar.toml
/// 4. /etc/meshstellar/meshstellar.toml
///
pub fn optional_config_file() -> Option<PathBuf> {
    let file_name = "meshstellar.toml".to_string();

    // File in $PWD
    let config: Option<PathBuf> =
        Some(PathBuf::from(&file_name)).filter(|path: &PathBuf| path.exists());
    if config.is_some() {
        return config;
    }

    // File in config directory
    // Path: $HOME/.config/meshstellar/meshstellar.toml.
    let config = dirs::config_dir()
        .map(|path| path.join("meshstellar").join(&file_name))
        .filter(|path| path.exists());
    if config.is_some() {
        return config;
    };

    // File in home directory
    // Path: $HOME/.meshstellar.toml.
    let hidden_name = format!(".{file_name}");
    let config = dirs::home_dir()
        .map(|path| path.join(hidden_name))
        .filter(|path| path.exists());
    if config.is_some() {
        return config;
    };

    if !cfg!(windows) {
        // Etc config
        // Path: /etc/meshstellar/meshstellar.toml
        let config = Some(PathBuf::from("/etc/meshstellar"))
            .map(|path: PathBuf| path.join(&file_name))
            .filter(|path| path.exists());
        if config.is_some() {
            return config;
        };
    }

    None
}

fn build_config() -> Result<Config> {
    let builder = Config::builder()
        .set_default("http_addr", "127.0.0.1:3000")?
        .set_default("database_url", "sqlite://meshstellar.db?mode=rwc")?
        .set_default("mqtt_port", 1883)?
        .set_default("mqtt_keep_alive", 15)?
        .set_default("mqtt_auth", true)?
        .set_default("mqtt_client_id", "meshstellar")?
        .set_default("mqtt_topic", "meshtastic/#")?
        .set_default(
            "map_glyphs_url",
            "https://protomaps.github.io/basemaps-assets/fonts/{fontstack}/{range}.pbf",
        )?
        .set_default("open_browser", true)?
        .set_default("hide_private_messages", false)?;

    Ok(if let Some(config_file) =
        optional_config_file().and_then(|path: PathBuf| path.to_str().map(|s| s.to_owned()))
    {
        info!("Loading configuration from {}", config_file);
        builder.add_source(config::File::new(&config_file, FileFormat::Toml).required(false))
    } else {
        if std::env::var("MESHSTELLAR_MQTT_HOST").is_err() {
            warn!("No configuration found, falling back to defaults. Create a configuration file or set the appropriate environment variables.");
        }

        builder
    }
    .add_source(config::Environment::with_prefix("MESHSTELLAR"))
    .build()?)
}
