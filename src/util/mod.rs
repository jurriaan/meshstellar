use rumqttc::{
    AsyncClient, ConnAck, ConnectReturnCode, Event, EventLoop, Incoming, MqttOptions, QoS, SubAck,
};
use sqlx::{migrate::Migrator, Executor, Pool, SqlitePool};
use std::{num::ParseIntError, time::Duration};
use tokio::time::timeout;
use tracing::{info, Level};
use tracing_subscriber::{
    fmt::writer::MakeWriterExt, layer::SubscriberExt, util::SubscriberInitExt,
};

pub mod config;
pub mod database_error;
pub mod plot;
pub mod static_file;
pub mod windows;

pub use database_error::DatabaseError;

pub type DB = sqlx::Sqlite;

static MIGRATOR: Migrator = sqlx::migrate!(); // defaults to "./migrations"

pub async fn connect_to_db() -> anyhow::Result<SqlitePool> {
    let database_url = config::get_config().get_string("database_url")?;

    let sqlx_options = sqlx::pool::PoolOptions::<DB>::new().after_connect(|conn, _meta| {
        Box::pin(async move {
            let statements = vec![
                "PRAGMA foreign_keys=ON;",
                "PRAGMA journal_mode = WAL;",
                "PRAGMA synchronous = NORMAL;",
                "PRAGMA busy_timeout = 15000;",
            ];

            for statement in statements {
                conn.execute(statement).await?;
            }

            Ok(())
        })
    });

    let sqlx_pool: Pool<DB> = sqlx_options.connect(&database_url).await?;
    MIGRATOR.run(&sqlx_pool).await?;

    return Ok(sqlx_pool);
}

pub fn setup_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::new()
                .with_writer(std::io::stdout.with_max_level(Level::INFO))
                .compact(),
        )
        .init();
}

pub async fn connect_to_mqtt() -> anyhow::Result<EventLoop> {
    let mqtt_host = config::get_config().get_string("mqtt_host")?;
    let mqtt_port = config::get_config().get_int("mqtt_port")?.try_into()?;
    let mqtt_keep_alive = config::get_config()
        .get_int("mqtt_keep_alive")?
        .try_into()?;
    let mqtt_auth = config::get_config().get_bool("mqtt_auth")?;
    let mqtt_client_id = config::get_config().get_string("mqtt_client_id")?;
    let mqtt_topic = config::get_config().get_string("mqtt_topic")?;

    let mut mqttoptions = MqttOptions::new(mqtt_client_id, &mqtt_host, mqtt_port);
    mqttoptions.set_keep_alive(Duration::from_secs(mqtt_keep_alive));

    if mqtt_auth {
        let mqtt_username = config::get_config().get_string("mqtt_username")?;
        let mqtt_password = config::get_config().get_string("mqtt_password")?;
        mqttoptions.set_credentials(mqtt_username, mqtt_password);
    }

    let (client, eventloop) = AsyncClient::new(mqttoptions, 10);
    client.subscribe(&mqtt_topic, QoS::AtLeastOnce).await?;

    let mqtt_connect_timeout = tokio::time::Duration::from_millis(30000);

    let result = timeout(mqtt_connect_timeout, wait_for_connection(eventloop)).await?;
    info!("MQTT subscribed to {}", mqtt_topic);
    result
}

async fn wait_for_connection(mut eventloop: EventLoop) -> anyhow::Result<EventLoop> {
    loop {
        let event = eventloop.poll().await?;

        if let Event::Incoming(Incoming::ConnAck(ConnAck {
            session_present: _,
            code: ConnectReturnCode::Success,
        })) = event
        {
            info!("MQTT connected");
        }

        if let Event::Incoming(Incoming::SubAck(SubAck {
            pkid: _,
            return_codes: _,
        })) = event
        {
            return Ok(eventloop);
        }
    }
}

pub fn stringify(x: ParseIntError) -> String {
    format!("error: {x}")
}

pub fn demoji(string: &str) -> String {
    string
        .chars()
        .filter(|&c| {
            // Emoticons
            !('\u{01F600}'..='\u{01F64F}').contains(&c) &&
            // Symbols & pictographs
            !('\u{01F300}'..='\u{01F5FF}').contains(&c) &&
            // Transport & map symbols
            !('\u{01F680}'..='\u{01F6FF}').contains(&c) &&
            // Flags
            !('\u{01F1E0}'..='\u{01F1FF}').contains(&c) &&
            // Dingbats
            !('\u{02702}'..='\u{027B0}').contains(&c) &&
            // Enclosed characters
            !('\u{02500}'..='\u{02BEF}').contains(&c)
        })
        .collect()
}

pub fn none_if_default<T>(value: T) -> Option<T>
where
    T: Default + Eq,
{
    if value == T::default() {
        None
    } else {
        Some(value)
    }
}
