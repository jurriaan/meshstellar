use chrono::Utc;
use rumqttc::{ConnAck, ConnectReturnCode, Event, Incoming, QoS, SubAck};
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::{dto::ServiceEnvelopeSelectResult, util::connect_to_mqtt};

pub async fn start_server(pool: SqlitePool) -> anyhow::Result<()> {
    let mqtt_topic = crate::util::config::get_config().get_string("mqtt_topic")?;
    info!("Starting MQTT processor");

    let (mut eventloop, client) = connect_to_mqtt().await?;

    loop {
        let event = eventloop.poll().await;
        match &event {
            Err(e) => {
                error!("MQTT error: {:?}", e);

                // Only retry every 5 seconds
                sleep(Duration::from_secs(5)).await;

                warn!("Retrying after MQTT error.");
            }
            Ok(Event::Incoming(Incoming::ConnAck(ConnAck {
                session_present,
                code: ConnectReturnCode::Success,
            }))) => {
                info!("Reconnected to broker!");

                if !session_present {
                    info!("Resubscribing to topic {}", mqtt_topic);
                    client.subscribe(&mqtt_topic, QoS::AtLeastOnce).await?;
                }
            }
            Ok(Event::Incoming(Incoming::SubAck(SubAck {
                pkid: _,
                return_codes: _,
            }))) => {
                info!("MQTT subscribed to {}", mqtt_topic);
            }
            Ok(Event::Incoming(Incoming::Publish(p))) => {
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
            event => {
                debug!("MQTT event: {:?}", event);
            }
        }
    }
}
