use anyhow::Result;
use std::sync::OnceLock;

use config::{Config, FileFormat};

pub fn get_config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();

    CONFIG.get_or_init(|| build_config().unwrap())
}

fn build_config() -> Result<Config> {
    Ok(Config::builder()
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
        .add_source(config::Environment::with_prefix("MESHSTELLAR"))
        .add_source(config::File::new("meshstellar.toml", FileFormat::Toml).required(false))
        .build()?)
}
