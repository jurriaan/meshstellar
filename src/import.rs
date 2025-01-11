use crate::{
    dto::{ReturningId, ServiceEnvelopeSelectResult},
    proto::{
        self,
        meshtastic::{
            mesh_packet::PayloadVariant::Decoded, routing, telemetry, MeshPacket, NeighborInfo,
            PortNum, Position, RouteDiscovery, Routing, ServiceEnvelope, Telemetry, User, Waypoint,
        },
    },
    util::{none_if_default, DB},
};
use anyhow::anyhow;

use chrono::Utc;
use prost::Message;
use sqlx::pool::PoolConnection;
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::{collections::HashSet, time::Duration};
use thiserror::Error;
use tokio::time;
use tokio_stream::{wrappers::IntervalStream, StreamExt};
use tracing::{error, info, warn};

#[derive(Debug, Clone, Error)]
#[error("Mesh packet processing error: {0}")]
struct MeshPacketProcessingError(String);

async fn ensure_node_exists(txn: &mut SqliteConnection, packet: &MeshPacket) -> anyhow::Result<()> {
    if packet.from != 0 && packet.from != 0xFFFFFFFF {
        let now = Utc::now().timestamp_nanos_opt().unwrap();
        let user_id = format!("!{:08x}", packet.from);
        let rx_time = packet.rx_time as i64 * 1_000_000_000;

        let _result = sqlx::query!(
            "INSERT INTO nodes (node_id, user_id, last_rx_time, last_rx_snr, last_rx_rssi, last_hop_start, last_hop_limit, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            ON CONFLICT(node_id) DO NOTHING",
            packet.from,
            user_id,
            rx_time,
            packet.rx_snr,
            packet.rx_rssi,
            packet.hop_start,
            packet.hop_limit,
            now
        )
        .execute(txn)
        .await?;
    }

    Ok(())
}

async fn process_mesh_packet(
    txn: &mut PoolConnection<DB>,
    gateway_id: String,
    raw_message_hash: &[u8],
    packet: &MeshPacket,
) -> anyhow::Result<()> {
    if let Some(Decoded(ref data)) = packet.payload_variant {
        let (mesh_repeat_id, node_exists) =
            fetch_mesh_repeat_and_node_exists(packet, txn, data, raw_message_hash).await?;

        if !node_exists {
            ensure_node_exists(txn, packet).await?;
        }

        let mesh_packet_id = create_packet(gateway_id, packet, data, raw_message_hash, txn).await?;

        if mesh_repeat_id != 0 {
            let _ = sqlx::query!(
                "UPDATE mesh_packets SET duplicate_of_mesh_packet_id = ? WHERE id = ?",
                mesh_repeat_id,
                mesh_packet_id,
            )
            .execute(&mut **txn)
            .await?;

            Err(anyhow!(MeshPacketProcessingError(format!(
                "Skipping processing of duplicate packet {}",
                mesh_packet_id
            ))))
        } else {
            let rx_time_nanos = packet.rx_time as i64 * 1_000_000_000;
            let now = Utc::now().timestamp_nanos_opt().unwrap();

            let _ = sqlx::query!(
                "UPDATE nodes
                 SET last_rx_time = ?, last_rx_snr = ?, last_rx_rssi = ?, last_hop_start = ?, last_hop_limit = ?, updated_at = ?
                 WHERE node_id = ?",
                rx_time_nanos,  // New last_rx_time value
                packet.rx_snr,  // New last_rx_snr value
                packet.rx_rssi, // New last_rx_rssi value
                packet.hop_start, // New last_hop_start value
                packet.hop_limit, // New last_hop_limit value
                now,
                packet.from,    // node_id condition
            )
            .execute(&mut **txn)
            .await?;

            match PortNum::try_from(data.portnum) {
                Ok(PortNum::PositionApp) => {
                    handle_position_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(PortNum::NeighborinfoApp) => {
                    handle_neighbor_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(PortNum::TelemetryApp) => {
                    handle_telemetry_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(PortNum::NodeinfoApp) => {
                    handle_nodeinfo_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(PortNum::WaypointApp) => {
                    handle_waypoint_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(PortNum::TracerouteApp) => {
                    handle_traceroute_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(PortNum::RoutingApp) => {
                    handle_routing_payload(data, packet, mesh_packet_id, txn).await
                }
                Ok(num) => Err(anyhow!(MeshPacketProcessingError(format!(
                    "Unsupported portnum: {:?}",
                    num
                )))),
                _ => Err(anyhow!(MeshPacketProcessingError(format!(
                    "Unknown portnum: {}",
                    data.portnum
                )))),
            }
        }?
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct PacketStatus {
    mesh_repeat_id: i64,
    exact_match: i64,
    node_exists: i64,
}

async fn fetch_mesh_repeat_and_node_exists(
    packet: &proto::meshtastic::MeshPacket,
    txn: &mut SqliteConnection,
    data: &proto::meshtastic::Data,
    raw_message_hash: &[u8],
) -> anyhow::Result<(i64, bool)> {
    let range_start = (packet.rx_time as i64 - 3600) * 1_000_000_000;
    let range_end = (packet.rx_time as i64 + 3600) * 1_000_000_000;

    let result : PacketStatus = sqlx::query_as!(
        PacketStatus,
        r#"
            SELECT
                COALESCE((SELECT id FROM mesh_packets WHERE unique_id = ? AND unique_id != 0 AND payload_data = ? AND created_at BETWEEN ? AND ? ORDER BY created_at ASC LIMIT 1), 0) AS "mesh_repeat_id!",
                EXISTS(SELECT 1 FROM mesh_packets WHERE hash = ? AND unique_id != 0) AS "exact_match!",
                EXISTS(SELECT 1 FROM nodes WHERE node_id = ?) AS "node_exists!"
        "#,
        packet.id,
        data.payload,
        range_start,
        range_end,
        raw_message_hash,
        packet.from
    )
    .fetch_one(txn)
    .await?;

    let exact_match = result.exact_match != 0;
    let node_exists = result.node_exists != 0;

    if !exact_match {
        Ok((result.mesh_repeat_id, node_exists))
    } else {
        Err(anyhow!("Duplicate mesh packet: {}", packet.id))
    }
}

async fn handle_position_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> Result<(), anyhow::Error> {
    if let Ok(position_payload) = Position::decode(&*data.payload) {
        if position_payload.latitude_i.is_some() && position_payload.longitude_i.is_some() {
            let timestamp = none_if_default(position_payload.timestamp).map(|_| {
                position_payload.timestamp as i64 * 1000000000
                    + position_payload.timestamp_millis_adjust as i64 * 1000000
            });

            let latitude = position_payload.latitude_i.map(|i| i as f64 / 1e7);
            let longitude = position_payload.longitude_i.map(|i| i as f64 / 1e7);
            let time = position_payload.time as i64 * 1_000_000_000;

            let result = sqlx::query_as!(
                ReturningId,
                "INSERT INTO positions (
                    mesh_packet_id, node_id, latitude, longitude, latitude_i, longitude_i,
                    altitude, time, location_source, altitude_source, timestamp, altitude_hae,
                    altitude_geoidal_separation, pdop, hdop, vdop, gps_accuracy, ground_speed,
                    ground_track, fix_quality, fix_type, sats_in_view, sensor_id, next_update,
                    seq_number, precision_bits
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                RETURNING id",
                mesh_packet_id,
                packet.from,
                latitude,
                longitude,
                position_payload.latitude_i,
                position_payload.longitude_i,
                position_payload.altitude,
                time,
                position_payload.location_source,
                position_payload.altitude_source,
                timestamp,
                position_payload.altitude_hae,
                position_payload.altitude_geoidal_separation,
                position_payload.pdop,
                position_payload.hdop,
                position_payload.vdop,
                position_payload.gps_accuracy,
                position_payload.ground_speed,
                position_payload.ground_track,
                position_payload.fix_quality,
                position_payload.fix_type,
                position_payload.sats_in_view,
                position_payload.sensor_id,
                position_payload.next_update,
                position_payload.seq_number,
                position_payload.precision_bits
            )
            .fetch_one(&mut **txn)
            .await?;

            let _ = sqlx::query!(
                "UPDATE nodes
                SET latitude = ?, longitude = ?, altitude = ?, last_position_id = ?
                WHERE node_id = ?",
                latitude,
                longitude,
                position_payload.altitude,
                result.id,
                packet.from,
            )
            .execute(&mut **txn)
            .await?;
        }
    };
    Ok(())
}

async fn handle_neighbor_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> Result<(), anyhow::Error> {
    let neighbor_info = NeighborInfo::decode(&*data.payload);
    if let Ok(neighbor_info) = neighbor_info {
        let node_ids: HashSet<u32> = neighbor_info.neighbors.iter().map(|n| n.node_id).collect();

        create_nodes_if_not_exist(node_ids, txn).await?;

        for neighbor_node in neighbor_info.neighbors {
            if neighbor_node.node_id != 0 && neighbor_node.node_id != 0xFFFFFFFF {
                let rx_time = packet.rx_time as i64 * 1_000_000_000;

                // Insert neighbor relationship
                let _ = sqlx::query!(
                    "INSERT INTO neighbors (mesh_packet_id, node_id, neighbor_node_id, snr, timestamp)
                    VALUES (?, ?, ?, ?, ?)",
                    mesh_packet_id,
                    neighbor_info.node_id,
                    neighbor_node.node_id,
                    neighbor_node.snr,
                    rx_time,
                )
                .execute(&mut **txn)
                .await?;
            }
        }
    };
    Ok(())
}

async fn handle_telemetry_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> Result<(), anyhow::Error> {
    if let Ok(telemetry_payload) = Telemetry::decode(&*data.payload) {
        let time = none_if_default(telemetry_payload.time).map(|time| time as i64 * 1000000000);

        match telemetry_payload.variant {
            Some(telemetry::Variant::DeviceMetrics(device_metrics_payload)) => {
                let result = sqlx::query_as!(
                    ReturningId,
                    "INSERT INTO device_metrics (mesh_packet_id, node_id, time, battery_level, voltage, air_util_tx, channel_utilization, uptime_seconds)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                     RETURNING id",
                    mesh_packet_id,
                    packet.from,
                    time,
                    device_metrics_payload.battery_level,
                    device_metrics_payload.voltage,
                    device_metrics_payload.air_util_tx,
                    device_metrics_payload.channel_utilization,
                    device_metrics_payload.uptime_seconds,
                )
                .fetch_one(&mut **txn)
                .await?;

                // Update nodes with device metrics
                let _ = sqlx::query!(
                    "UPDATE nodes SET battery_level = ?, voltage = ?, air_util_tx = ?, channel_utilization = ?, uptime_seconds = ?, last_device_metrics_id = ?
                    WHERE node_id = ?",
                    device_metrics_payload.battery_level,
                    device_metrics_payload.voltage,
                    device_metrics_payload.air_util_tx,
                    device_metrics_payload.channel_utilization,
                    device_metrics_payload.uptime_seconds,
                    result.id,
                    packet.from,
                )
                .execute(&mut **txn)
                .await?;
            }
            Some(telemetry::Variant::EnvironmentMetrics(environment_metrics_payload)) => {
                let result = sqlx::query_as!(
                    ReturningId,
                    "INSERT INTO environment_metrics (mesh_packet_id, node_id, time, temperature, relative_humidity, barometric_pressure, gas_resistance, iaq)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                     RETURNING id",
                    mesh_packet_id,
                    packet.from,
                    time,
                    environment_metrics_payload.temperature,
                    environment_metrics_payload.relative_humidity,
                    environment_metrics_payload.barometric_pressure,
                    environment_metrics_payload.gas_resistance,
                    environment_metrics_payload.iaq,
                )
                .fetch_one(&mut **txn)
                .await?;

                // Update nodes with environment metrics
                let _ = sqlx::query!(
                    "UPDATE nodes SET temperature = ?, relative_humidity = ?, barometric_pressure = ?, gas_resistance = ?, iaq = ?, last_environment_metrics_id = ?
                    WHERE node_id = ?",
                    environment_metrics_payload.temperature,
                    environment_metrics_payload.relative_humidity,
                    environment_metrics_payload.barometric_pressure,
                    environment_metrics_payload.gas_resistance,
                    environment_metrics_payload.iaq,
                    result.id,
                    packet.from,
                )
                .execute(&mut **txn)
                .await?;
            }
            Some(telemetry::Variant::PowerMetrics(power_metrics_payload)) => {
                let _ = sqlx::query_as!(
                    ReturningId,
                    "INSERT INTO power_metrics (mesh_packet_id, node_id, time, ch1_voltage, ch1_current, ch2_voltage, ch2_current, ch3_voltage, ch3_current)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                     RETURNING id",
                    mesh_packet_id,
                    packet.from,
                    time,
                    power_metrics_payload.ch1_voltage,
                    power_metrics_payload.ch1_current,
                    power_metrics_payload.ch2_voltage,
                    power_metrics_payload.ch2_current,
                    power_metrics_payload.ch3_voltage,
                    power_metrics_payload.ch3_current,
                )
                .fetch_one(&mut **txn)
                .await?;

                // Not stored in nodes table currently
            }
            _ => {
                println!("Telemetry{:?}", telemetry_payload);
            }
        }
    };
    Ok(())
}

async fn handle_nodeinfo_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> Result<(), anyhow::Error> {
    if let Ok(node_info_payload) = User::decode(&*data.payload) {
        let result = sqlx::query_as!(
            ReturningId,
            "INSERT INTO node_info (mesh_packet_id, node_id, user_id, long_name, short_name, hw_model_id, is_licensed, role)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            mesh_packet_id,
            packet.from,
            node_info_payload.id,
            node_info_payload.long_name,
            node_info_payload.short_name,
            node_info_payload.hw_model,
            node_info_payload.is_licensed,
            node_info_payload.role,
        )
        .fetch_one(&mut **txn)
        .await?;

        // Update nodes table
        let _ = sqlx::query!(
            "UPDATE nodes
            SET user_id = ?, long_name = ?, short_name = ?, hw_model_id = ?, is_licensed = ?, role = ?, last_node_info_id = ?
            WHERE node_id = ?",
            node_info_payload.id,
            node_info_payload.long_name,
            node_info_payload.short_name,
            node_info_payload.hw_model,
            node_info_payload.is_licensed,
            node_info_payload.role,
            result.id,
            packet.from,
        )
        .execute(&mut **txn)
        .await?;
    };
    Ok(())
}

async fn handle_waypoint_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> anyhow::Result<()> {
    if let Ok(waypoint_payload) = Waypoint::decode(&*data.payload) {
        let expire =
            none_if_default(waypoint_payload.expire as i64).map(|expire| expire * 1_000_000_000);
        let locked_to = none_if_default(waypoint_payload.locked_to);
        let icon = if waypoint_payload.icon == 0 {
            String::new()
        } else {
            char::from_u32(waypoint_payload.icon)
                .map(|c| c.to_string())
                .unwrap_or_default()
        };

        let latitude = waypoint_payload.latitude_i.map(|i| i as f64 / 1e7);
        let longitude = waypoint_payload.longitude_i.map(|i| i as f64 / 1e7);

        sqlx::query!(
            "INSERT INTO waypoints (
                mesh_packet_id, node_id, waypoint_id, latitude, longitude, latitude_i, longitude_i,
                expire, locked_to, name, description, icon
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            mesh_packet_id,
            packet.from,
            waypoint_payload.id,
            latitude,
            longitude,
            waypoint_payload.latitude_i,
            waypoint_payload.longitude_i,
            expire,
            locked_to,
            waypoint_payload.name,
            waypoint_payload.description,
            icon
        )
        .execute(&mut **txn)
        .await?;
    };
    Ok(())
}

async fn handle_traceroute_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    _mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> anyhow::Result<()> {
    if let Ok(route_discovery_payload) = RouteDiscovery::decode(&*data.payload) {
        let mut node_ids = HashSet::from_iter(route_discovery_payload.route);
        node_ids.insert(packet.to);

        create_nodes_if_not_exist(node_ids, txn).await?;

        // Traceroutes are parsed on the fly currently, no database entry will be created.
    };
    Ok(())
}

async fn handle_routing_payload(
    data: &proto::meshtastic::Data,
    packet: &proto::meshtastic::MeshPacket,
    _mesh_packet_id: i64,
    txn: &mut PoolConnection<DB>,
) -> anyhow::Result<()> {
    if let Ok(routing_payload) = Routing::decode(&*data.payload) {
        let variant = routing_payload
            .variant
            .ok_or(anyhow!("Unknown routing variant"))?;

        if let routing::Variant::RouteRequest(discovery) | routing::Variant::RouteReply(discovery) =
            variant
        {
            let mut node_ids = HashSet::from_iter(discovery.route);
            node_ids.insert(packet.to);

            create_nodes_if_not_exist(node_ids, txn).await?;
        }

        // Routing messages are parsed on the fly currently, no database entry will be created.
    };
    Ok(())
}

async fn create_nodes_if_not_exist(
    node_ids: HashSet<u32>,
    txn: &mut PoolConnection<sqlx::Sqlite>,
) -> Result<(), anyhow::Error> {
    if node_ids.is_empty() {
        return Ok(());
    };

    let query = format!(
        "SELECT node_id FROM nodes WHERE node_id IN ({})",
        node_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
    );
    let mut query = sqlx::query(&query);
    for node_id in &node_ids {
        query = query.bind(node_id);
    }
    let existing_nodes: HashSet<u32> = query
        .fetch_all(&mut **txn)
        .await?
        .into_iter()
        .map(|row| row.get::<u32, _>("node_id"))
        .collect();
    for node_id in node_ids.difference(&existing_nodes) {
        if *node_id != 0 && *node_id != 0xFFFFFFFF {
            // Attempt to insert the node, ignoring conflicts
            let user_id = format!("!{:08x}", node_id);
            let _ = sqlx::query!(
                "INSERT INTO nodes (node_id, user_id) VALUES (?, ?) ON CONFLICT(node_id) DO NOTHING",
                node_id, user_id
            )
            .execute(&mut **txn)
            .await;
        }
    }
    Ok(())
}

async fn create_packet(
    gateway_id: String,
    packet: &MeshPacket,
    data: &proto::meshtastic::Data,
    raw_message_hash: &[u8],
    txn: &mut SqliteConnection,
) -> anyhow::Result<i64> {
    let now = Utc::now().timestamp_nanos_opt().unwrap();
    let source = none_if_default(data.source as i64);
    let dest = none_if_default(data.dest as i64);
    let request_id = none_if_default(data.request_id as i64);
    let reply_id = none_if_default(data.reply_id as i64);
    let emoji = none_if_default(data.emoji as i64);
    let rx_time = packet.rx_time as i64 * 1_000_000_000;

    let result = sqlx::query_as!(
        ReturningId,
        "INSERT INTO mesh_packets (
            gateway_id, from_id, to_id, channel_id, unique_id, portnum,
            payload_data, rx_time, rx_snr, rx_rssi, hop_start, hop_limit,
            want_ack, want_response, source, dest, request_id, reply_id,
            emoji, priority, hash, created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
        RETURNING id",
        gateway_id,
        packet.from,
        packet.to,
        packet.channel,
        packet.id,
        data.portnum,
        data.payload,
        rx_time,
        packet.rx_snr,
        packet.rx_rssi,
        packet.hop_start,
        packet.hop_limit,
        packet.want_ack,
        data.want_response,
        source,
        dest,
        request_id,
        reply_id,
        emoji,
        packet.priority,
        raw_message_hash,
        now
    )
    .fetch_one(txn)
    .await?;

    Ok(result.id)
}

async fn handle_raw_service_envelope(
    txn: &mut PoolConnection<DB>,
    raw_message_hash: &[u8],
    service_envelope: &[u8],
) -> anyhow::Result<()> {
    if let Ok(message) = ServiceEnvelope::decode(service_envelope) {
        if let Some(packet) = message.packet {
            process_mesh_packet(txn, message.gateway_id, raw_message_hash, &packet).await?;
        }
    }

    Ok(())
}

pub async fn start_server(pool: SqlitePool) -> anyhow::Result<()> {
    info!("Starting import server");

    let mut stream = IntervalStream::new(time::interval(Duration::from_secs(1)));

    while (stream.next().await).is_some() {
        process_service_envelopes(&pool).await?;
    }

    Ok(())
}

async fn process_service_envelopes(pool: &SqlitePool) -> Result<(), anyhow::Error> {
    let entities = sqlx::query_as!(
        ServiceEnvelopeSelectResult,
        "SELECT id, hash, payload_data FROM service_envelopes WHERE processed_at IS NULL ORDER BY created_at"
    ).fetch_all(pool)
        .await?;

    for service_envelope in entities {
        let mut txn = pool.acquire().await?;
        sqlx::query!("BEGIN IMMEDIATE").execute(&mut *txn).await?;
        process_service_envelope(&mut txn, service_envelope).await?;
        sqlx::query!("COMMIT").execute(&mut *txn).await?;
    }

    Ok(())
}

pub async fn process_service_envelope(
    txn: &mut PoolConnection<DB>,
    service_envelope: ServiceEnvelopeSelectResult,
) -> Result<(), anyhow::Error> {
    let result =
        handle_raw_service_envelope(txn, &service_envelope.hash, &service_envelope.payload_data)
            .await;
    if let Err(err) = result {
        if let Some(MeshPacketProcessingError(_)) = err.downcast_ref() {
            warn!("Skipping packet after processing error: {}", err);
        }
    } else {
        result?;
    }

    // Get the current timestamp in nanoseconds
    let processed_at = chrono::Utc::now().timestamp_nanos_opt();

    // Update the processed_at field for the given service_envelope_id
    sqlx::query!(
        "UPDATE service_envelopes SET processed_at = ? WHERE id = ?",
        processed_at,
        service_envelope.id
    )
    .execute(&mut **txn)
    .await?;

    Ok(())
}
