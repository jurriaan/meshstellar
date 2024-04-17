use chrono::Utc;
use rumqttc::{Event, Incoming};
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info};

use crate::{dto::ServiceEnvelopeSelectResult, util::connect_to_mqtt};

pub async fn start_server(pool: SqlitePool) -> anyhow::Result<()> {
    info!("Starting MQTT processor");

    let mut eventloop = connect_to_mqtt().await?;

    loop {
        let event = eventloop.poll().await;

        if let Err(e) = &event {
            error!("MQTT error: {:?}", e);

            // Only retry every 5 seconds
            sleep(Duration::from_secs(5)).await;
        } else if let Ok(Event::Incoming(Incoming::Publish(p))) = &event {
            let raw_message = p.payload.to_vec();

            let raw_message_hash = blake3::hash(&raw_message).as_bytes().to_vec();
            let created_at = Utc::now().timestamp_nanos_opt().unwrap();
            let _service_envelope: ServiceEnvelopeSelectResult = sqlx::query_as!(
                ServiceEnvelopeSelectResult,
                "INSERT INTO service_envelopes (payload_data, hash, created_at)
                 VALUES (?, ?, ?)
                 RETURNING id, hash, payload_data",
                raw_message,
                raw_message_hash, // Convert hash to bytes if not already in this format
                created_at,
            )
            .fetch_one(&pool)
            .await?;
        }
    }
}
