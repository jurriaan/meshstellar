use crate::{
    dto::{
        mesh_packet::Payload, DeviceMetricsSelectResult, EnvironmentMetricsSelectResult,
        GatewayPacketInfo, MeshPacket as MeshPacketDto, NeighborSelectResult, NodeSelectResult,
        PlotData, PositionSelectResult, PowerMetricsSelectResult, RoutingDto, StatsSelectResult,
        TracerouteDto, WaypointSelectResult,
    },
    proto::meshtastic::{routing, PortNum, RouteDiscovery, Routing},
    template::*,
    util::{
        self, capitalize,
        config::get_config,
        demoji,
        static_file::{Asset, StaticFile},
        stringify,
        template::into_response,
        DatabaseError,
    },
};
use askama::Template;
use async_stream::stream;
use axum::http::HeaderMap;
use axum::{
    extract::{FromRef, Path, State},
    http::{header, Uri},
    response::{
        sse::{Event, Sse},
        Html, IntoResponse,
    },
    routing::get,
    Router,
};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use futures::{
    stream::{select_all, Stream},
    FutureExt,
};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, JsonObject, JsonValue};
use itertools::Itertools;
use prost::Message;
use serde::Deserialize;
use serde_json::{json, Map};
use sqlx::{FromRow, SqlitePool};
use std::{collections::HashMap, sync::OnceLock};
use std::{convert::Infallible, pin::Pin, time::Duration};
use tokio::net::TcpListener;
use tokio::sync::OnceCell;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::{error, info};

#[derive(Clone)]
struct WebConfig {
    pmtiles_url: Option<String>,
    map_glyphs_url: String,
    hide_private_messages: bool,
}

// Define your application shared state
#[derive(Clone, FromRef)]
struct AppState {
    pool: SqlitePool,
    web_config: WebConfig,
}

async fn index() -> impl IntoResponse {
    into_response(&IndexTemplate {})
}

const NODENUM_BROADCAST: u32 = 0xffffffff;

fn stats_update_stream(
    pool: State<SqlitePool>,
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    Box::pin(stream! {
        loop {
            let result = sqlx::query_as!(
                StatsSelectResult,
                r#"SELECT
                    COALESCE((SELECT COUNT(id) FROM mesh_packets), 0) AS "num_packets!",
                    COALESCE((SELECT COUNT(id) FROM nodes), 0) AS "num_nodes!"
                "#
            )
            .fetch_one(&*pool)
            .await;

            if let Err(err) = result {
                error!("Error occurred while fetching stats: {}", err);
                break;
            }
            let template = StatsTemplate {
                stats: result.unwrap_or_default(),
            };

                if let Ok(data) = template.render() {
                    yield Ok(Event::default().event("statistics").data(data))
                }

            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    })
}

fn node_update_stream(
    pool: State<SqlitePool>,
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    let mut last_updated_at: i64 = 0;

    Box::pin(stream! {
        loop {
            let nodes = sqlx::query_as!(
                NodeSelectResult,
                r#"
                WITH neighbor_data AS (
                    SELECT node_id, json_object('neighbor', printf('%x',neighbor_node_id), 'snr', snr, 'timestamp', timestamp/1000000000) AS neighbor
                    FROM
                        (
                            SELECT
                                node_id,
                                neighbor_node_id,
                                snr,
                                timestamp,
                                row_number() over (partition by node_id, neighbor_node_id
                            ORDER BY
                                timestamp DESC) AS row_number
                            FROM
                                neighbors
                            WHERE
                                timestamp > (strftime('%s', 'now') * 1000000000) - (8 * 60 * 60 * 1000000000)
                        )
                        a
                    WHERE
                        row_number = 1 ORDER BY timestamp DESC
                ),
                neighbors_per_node AS (
                    SELECT node_id, json_group_array(json(neighbor)) AS neighbor_json FROM neighbor_data GROUP BY 1
                )
                SELECT
                    nodes.node_id,
                    user_id,
                    last_rx_time,
                    last_rx_snr,
                    last_rx_rssi,
                    last_hop_start,
                    last_hop_limit,
                    long_name,
                    short_name,
                    hw_model_id,
                    is_licensed,
                    role,
                    battery_level,
                    voltage,
                    channel_utilization,
                    air_util_tx,
                    uptime_seconds,
                    temperature,
                    relative_humidity,
                    barometric_pressure,
                    gas_resistance,
                    iaq,
                    latitude,
                    longitude,
                    altitude,
                    COALESCE(neighbor_json, '[]') AS neighbor_json,
                    updated_at
                FROM nodes
                LEFT JOIN neighbors_per_node ON neighbors_per_node.node_id = nodes.node_id
                WHERE updated_at > ?
                ORDER BY updated_at ASC
                "#,
                last_updated_at
            )
            .fetch_all(&*pool)
            .await;

            if let Err(err) = nodes {
                error!("Error occurred while fetching nodes: {}", err);
                break;
            }

            let nodes = nodes.unwrap_or_default();

            last_updated_at = nodes.last().map(|n| n.updated_at).unwrap_or(last_updated_at);

            for node in nodes.into_iter() {
                let geom = if let (Some(longitude), Some(latitude), Some(altitude)) = (node.longitude, node.latitude, node.altitude) {
                    let geometry: Geometry = geojson::Value::Point(vec![longitude, latitude, altitude as f64]).into();
                    let mut properties = JsonObject::new();
                    let short_name = demoji(node.short_name.clone().unwrap_or_default().as_str());
                    let display_name = match short_name.trim() {
                        "" => node.user_id.clone(),
                        _ => short_name
                    };

                    properties.insert("id".to_string(), JsonValue::from(format!("{:x}", node.node_id)));
                    properties.insert("display_name".to_string(), JsonValue::from(display_name));
                    properties.insert("long_name".to_string(), JsonValue::from(node.long_name.clone()));
                    properties.insert("updated_at".to_string(), JsonValue::from(node.updated_at as f64 / 1_000_000_000.0));

                    Some(GeoJson::Feature(Feature {
                        geometry: Some(geometry),
                        properties: Some(properties),
                        ..Default::default()
                    }).to_string())
                } else {
                    None
                };

                let template = NodeTemplate { node, geom };

                if let Ok(data) = template.render() {
                    yield Ok(Event::default().event("update-node").data(data))
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

fn mesh_packet_stream(
    pool: State<SqlitePool>,
    hide_private_messages: bool,
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    let mut last_id: i64 = 0;

    Box::pin(stream! {
        // Delay by half a second to allow nodes to load first
        tokio::time::sleep(Duration::from_millis(500)).await;

        loop {
            let packets : Result<Vec<MeshPacketDto>, sqlx::Error> = sqlx::query_as(
                r#"
                SELECT
                    id,
                    from_id,
                    to_id,
                    gateway_id,
                    portnum,
                    rx_time,
                    hop_start,
                    hop_limit,
                    rx_snr,
                    rx_rssi,
                    priority,
                    want_ack,
                    want_response,
                    payload_data,
                    created_at
                FROM mesh_packets
                WHERE id IN (
                    SELECT id FROM mesh_packets
                    WHERE id > ?1
                    AND duplicate_of_mesh_packet_id IS NULL
                    ORDER BY id DESC
                    LIMIT 100
                ) OR id IN (
                    SELECT id FROM mesh_packets
                    WHERE id > ?1 AND portnum = ?2
                    AND duplicate_of_mesh_packet_id IS NULL
                    ORDER BY id DESC
                    LIMIT 100
                )
                ORDER BY id DESC
                "#,
            )
            .bind(last_id)
            .bind(Into::<i32>::into(PortNum::TextMessageApp))
            .fetch_all(&*pool)
                .await;

            if let Err(err) = packets {
                error!("Error occurred while fetching packets: {}", err);
                break;
            }

            let packets = packets.unwrap_or_default();

            let packet_ids = packets.iter().map(|p| p.id).collect_vec();
            let packet_ids_string = packet_ids.iter().join(",");

            let compute_waypoints = || {
                async {
                let query = format!(
                    r#"SELECT mesh_packet_id, latitude, longitude, expire, locked_to, name, description, icon FROM waypoints WHERE mesh_packet_id IN ({})"#,
                    packet_ids_string
                );
                sqlx::query_as::<_, WaypointSelectResult>(&query)
                .fetch_all(&*pool)
                .map(|waypoints|
                    if let Ok(waypoints) = waypoints {
                        waypoints.into_iter().map(|wp| (wp.mesh_packet_id, wp)).collect()
                    } else {
                        Default::default()
                    }
                ).await
            }
            };
            let compute_positions = || {
                async {
                let query = format!(
                    r#"SELECT mesh_packet_id, latitude, longitude, altitude, sats_in_view, precision_bits, ground_speed, seq_number FROM positions WHERE mesh_packet_id IN ({})"#,
                     packet_ids_string
                );
                sqlx::query_as::<_, PositionSelectResult>(&query)
                .fetch_all(&*pool)
                 .map(|positions|
                    if let Ok(positions) = positions {
                        positions.into_iter().map(|p| (p.mesh_packet_id, p)).collect()
                    } else {
                        Default::default()
                    }
                 ).await
                }
            };
            let compute_device_metrics = || {
                async {
                let query = format!(
                    r#"SELECT mesh_packet_id, time, battery_level, voltage, channel_utilization, air_util_tx, uptime_seconds FROM device_metrics WHERE mesh_packet_id IN ({})"#,
                    packet_ids_string
                );
                sqlx::query_as::<_, DeviceMetricsSelectResult>(&query)
                .fetch_all(&*pool)
                 .map(|metrics|
                    if let Ok(metrics) = metrics {
                        metrics.into_iter().map(|p| (p.mesh_packet_id, p)).collect()
                    } else {
                        Default::default()
                    }
                 ).await
                }
            };
            let compute_environment_metrics = || {
                async {
                let query = format!(
                    r#"SELECT mesh_packet_id, time, temperature, relative_humidity, barometric_pressure, gas_resistance, iaq FROM environment_metrics WHERE mesh_packet_id IN ({})"#,
                     packet_ids_string
                );
                sqlx::query_as::<_, EnvironmentMetricsSelectResult>(&query)
                .fetch_all(&*pool)
                 .map(|metrics|
                    if let Ok(metrics) = metrics {
                        metrics.into_iter().map(|p| (p.mesh_packet_id, p)).collect()
                    } else {
                        Default::default()
                    }
                 ).await
                }
            };
            let compute_power_metrics = || {
                async {
                let query = format!(
                    r#"SELECT mesh_packet_id, time, ch1_voltage, ch1_current, ch2_voltage, ch2_current, ch3_voltage, ch3_current FROM power_metrics WHERE mesh_packet_id IN ({})"#,
                     packet_ids_string
                );
                sqlx::query_as::<_, PowerMetricsSelectResult>(&query)
                .fetch_all(&*pool)
                 .map(|metrics|
                    if let Ok(metrics) = metrics {
                        metrics.into_iter().map(|p| (p.mesh_packet_id, p)).collect()
                    } else {
                        Default::default()
                    }
                 ).await
                }
            };
            let compute_neighbors = || {
                async {
                    let query = format!(
                        "SELECT mesh_packet_id, neighbor_node_id, snr FROM neighbors WHERE mesh_packet_id IN ({}) ORDER BY mesh_packet_id, id",
                        packet_ids_string
                    );
                    sqlx::query_as::<_, NeighborSelectResult>(&query)
                    .fetch_all(&*pool)
                    .map(|neighbors|
                        if let Ok(neighbors) = neighbors {
                            neighbors.into_iter().into_group_map_by(|p| p.mesh_packet_id)
                        } else {
                            Default::default()
                        }
                    ).await
                }
            };

            // This is a hack, let's improve..
            let waypoints : OnceCell<HashMap<i64, WaypointSelectResult>> = OnceCell::new();
            let positions : OnceCell<HashMap<i64, PositionSelectResult>> = OnceCell::new();
            let device_metrics : OnceCell<HashMap<i64, DeviceMetricsSelectResult>> = OnceCell::new();
            let environment_metrics: OnceCell<HashMap<i64, EnvironmentMetricsSelectResult>> = OnceCell::new();
            let power_metrics: OnceCell<HashMap<i64, PowerMetricsSelectResult>> = OnceCell::new();
            let neighbors: OnceCell<HashMap<i64, Vec<NeighborSelectResult>>> = OnceCell::new();

            last_id = packets.first().map(|p| p.id).unwrap_or(last_id);

            for mut packet in packets.into_iter().rev() {
                let mut event_type = "mesh-packet";
                let mut hide_packet = false;

                match PortNum::try_from(packet.portnum) {
                    Ok(PortNum::TextMessageApp) => {
                        hide_packet = hide_private_messages && packet.to_id != NODENUM_BROADCAST;
                        packet.payload = Payload::TextMessage(String::from_utf8(packet.payload_data.clone()).unwrap_or_default());
                        event_type = "text-message";
                    }
                    Ok(PortNum::WaypointApp) => {
                        if let Some(waypoint) = waypoints.get_or_init(compute_waypoints).await.get(&packet.id) {
                            packet.payload = Payload::Waypoint(waypoint.clone())
                        }
                    }
                    Ok(PortNum::PositionApp) => {
                        if let Some(position) = positions.get_or_init(compute_positions).await.get(&packet.id) {
                            packet.payload = Payload::Position(*position)
                        }
                    }
                    Ok(PortNum::TelemetryApp) => {
                        if let Some(device_metrics) = device_metrics.get_or_init(compute_device_metrics).await.get(&packet.id) {
                            packet.payload = Payload::DeviceMetrics(device_metrics.clone())
                        } else if let Some(environment_metrics) = environment_metrics.get_or_init(compute_environment_metrics).await.get(&packet.id) {
                            packet.payload = Payload::EnvironmentMetrics(environment_metrics.clone())
                        } else if let Some(power_metrics) = power_metrics.get_or_init(compute_power_metrics).await.get(&packet.id) {
                            packet.payload = Payload::PowerMetrics(power_metrics.clone())
                        }
                    }
                    Ok(PortNum::NeighborinfoApp) => {
                        if let Some(neighbors) = neighbors.get_or_init(compute_neighbors).await.get(&packet.id) {
                            packet.payload = Payload::Neighbors(neighbors.clone());
                        }
                    }
                    Ok(PortNum::TracerouteApp) => {
                        if let Ok(route_discovery) = RouteDiscovery::decode(&*packet.payload_data) {
                            let is_response = !packet.want_response;

                            packet.payload = Payload::Traceroute(if is_response {
                                TracerouteDto {
                                    from_id: packet.to_id,
                                    to_id: packet.from_id,
                                    is_response,
                                    route: route_discovery.route
                                }
                            } else {
                                TracerouteDto {
                                    from_id: packet.from_id,
                                    to_id: packet.to_id,
                                    is_response,
                                    route: route_discovery.route
                                }
                            });
                        }
                    }
                    Ok(PortNum::RoutingApp) => {
                        if let Ok(Some(routing_variant)) = Routing::decode(&*packet.payload_data).map(|r| r.variant) {
                            let (variant_name, error_reason) = match routing_variant {
                                routing::Variant::RouteRequest(_) => ("Route request", None),
                                routing::Variant::RouteReply(_) => ("Route reply", None),
                                routing::Variant::ErrorReason(error) => {
                                    let error = routing::Error::try_from(error).map(|error| capitalize(error.as_str_name().replace('_'," ").as_str()))
                                   .unwrap_or_else(|_| "Unknown".to_string());
                                    ("Error reason", Some(error))
                                },
                            };

                            packet.payload = Payload::Routing(RoutingDto {
                                variant_name: variant_name.to_string(),
                                error_reason
                            })
                        }
                    }
                    _ => {}
                }
                if !hide_packet {
                    let template = PacketTemplate { packet };

                    if let Ok(data) = template.render() {
                        yield Ok(Event::default().event(event_type).data(data))
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

async fn sse_handler(
    pool: State<SqlitePool>,
    web_config: State<WebConfig>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let update_stream = select_all(
        vec![
            node_update_stream(pool.clone()),
            mesh_packet_stream(pool.clone(), web_config.hide_private_messages),
            stats_update_stream(pool),
        ]
        .into_iter(),
    );

    Sse::new(update_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("alive"),
    )
}

impl FromIterator<MeshPacketDto> for FeatureCollection {
    fn from_iter<T: IntoIterator<Item = MeshPacketDto>>(iter: T) -> Self {
        let features =
            iter.into_iter()
                .filter_map(|packet| {
                    if let Payload::Position(pos) = packet.payload {
                        let coordinates = vec![pos.longitude, pos.latitude, pos.altitude as f64];
                        let geometry: Geometry = geojson::Value::Point(coordinates).into();
                        let id = geojson::feature::Id::Number(packet.id.into());
                        let template = PositionDetailsTemplate { packet };

                        let properties = template.render().ok().map(|result| {
                            Map::from_iter(vec![("desc".to_string(), result.into())])
                        });

                        Some(Feature {
                            id: Some(id),
                            geometry: Some(geometry),
                            properties,
                            foreign_members: None,
                            bbox: None,
                        })
                    } else {
                        None
                    }
                })
                .collect_vec();

        FeatureCollection {
            features,
            foreign_members: None,
            bbox: None,
        }
    }
}

#[derive(Deserialize)]
struct MaxAge {
    max_age: Option<u32>,
}

async fn node_positions_geojson(
    pool: State<SqlitePool>,
    Path((node_id,)): Path<(String,)>,
    max_age: axum::extract::Query<MaxAge>,
) -> axum::response::Result<impl IntoResponse> {
    let node_id = u32::from_str_radix(&node_id, 16).map_err(stringify)?;

    let max_age_time_nanos = if let Some(max_age) = max_age.max_age {
        let cur_time = Utc::now().timestamp_nanos_opt().unwrap();
        cur_time - (max_age as i64 * 60_000_000_000)
    } else {
        0
    };

    let query_res: Vec<MeshPacketDto> = sqlx::query(
        r#"
            SELECT
                mesh_packets.id,
                mesh_packets.from_id,
                mesh_packets.to_id,
                mesh_packets.gateway_id,
                mesh_packets.portnum,
                mesh_packets.rx_time,
                mesh_packets.hop_start,
                mesh_packets.hop_limit,
                mesh_packets.rx_snr,
                mesh_packets.rx_rssi,
                mesh_packets.priority,
                mesh_packets.want_ack,
                mesh_packets.want_response,
                mesh_packets.payload_data,
                mesh_packets.created_at,
                positions.mesh_packet_id,
                positions.latitude,
                positions.longitude,
                positions.altitude,
                positions.sats_in_view,
                positions.precision_bits,
                positions.ground_speed,
                positions.seq_number
            FROM positions
            JOIN mesh_packets ON positions.mesh_packet_id = mesh_packets.id
            WHERE node_id = ? AND created_at > ?
            ORDER BY created_at DESC
            LIMIT 250
            "#,
    )
    .bind(node_id)
    .bind(max_age_time_nanos)
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?
    .into_iter()
    .filter_map(|row| {
        MeshPacketDto::from_row(&row)
            .and_then(|mut packet| {
                PositionSelectResult::from_row(&row).map(|position| {
                    packet.payload = Payload::Position(position);
                    packet
                })
            })
            .ok()
    })
    .collect_vec();

    let res = GeoJson::FeatureCollection(query_res.into_iter().collect()).to_string();

    Ok(([(header::CONTENT_TYPE, "application/geo+json")], res))
}

#[derive(Deserialize)]
struct NodeDetailsQueryParams {
    gateway: Option<String>,
}

async fn node_details(
    pool: State<SqlitePool>,
    Path((node_id,)): Path<(String,)>,
    query: axum::extract::Query<NodeDetailsQueryParams>,
    headers: HeaderMap,
) -> axum::response::Result<impl IntoResponse> {
    let node_id = i64::from_str_radix(&node_id, 16).map_err(stringify)? as i64;
    // TODO: improve query to match neighbor node updates or setup new struct.
    let node = sqlx::query_as!(
        NodeSelectResult,
        r#"
        SELECT
            node_id,
            user_id,
            last_rx_time,
            last_rx_snr,
            last_rx_rssi,
            last_hop_start,
            last_hop_limit,
            long_name,
            short_name,
            hw_model_id,
            is_licensed,
            role,
            battery_level,
            voltage,
            channel_utilization,
            air_util_tx,
            uptime_seconds,
            temperature,
            relative_humidity,
            barometric_pressure,
            gas_resistance,
            iaq,
            latitude,
            longitude,
            altitude,
            '[]' AS neighbor_json,
            updated_at
        FROM nodes
        WHERE node_id = ?
        "#,
        node_id
    )
    .fetch_one(&*pool)
    .await
    .map_err(DatabaseError)?;

    let max_age_minutes: Option<i64> = headers
        .get("x-meshstellar-max-age")
        .map(|x| x.to_str().unwrap_or("all"))
        .and_then(|x| x.parse().ok());

    let min_time_nanos = if let Some(max_age) = max_age_minutes {
        let cur_time = Utc::now().timestamp_nanos_opt().unwrap();
        cur_time - (max_age * 60_000_000_000)
    } else {
        0
    };

    let offset_minutes: i32 = headers
        .get("x-meshstellar-tz-offset")
        .map(|x| x.to_str().unwrap_or("0"))
        .and_then(|x| x.parse().ok())
        .unwrap_or_default();
    let offset = if offset_minutes.abs() < 24 * 60 {
        FixedOffset::west_opt(offset_minutes * 60)
            .unwrap_or_else(|| FixedOffset::west_opt(0).unwrap())
    } else {
        FixedOffset::west_opt(0).unwrap()
    };

    let gateway_packet_info: Vec<GatewayPacketInfo> = sqlx::query_as!(
        GatewayPacketInfo,
        r#"
            SELECT gateway_id as "gateway_id!", COUNT(*) as num_packets FROM mesh_packets WHERE from_id = ?1 AND created_at > ?2 AND gateway_id IS NOT NULL GROUP BY 1 ORDER BY num_packets DESC LIMIT 50
        "#,
        node_id, min_time_nanos
    )
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?;

    let max_gateway = gateway_packet_info.first();

    if let Some(max_gateway) = max_gateway {
        // A bit too complicated way to find the gateway in the list..
        let selected_node = query
            .gateway
            .as_ref()
            .and_then(|selected| {
                gateway_packet_info
                    .iter()
                    .find(|g| &g.gateway_id == selected)
                    .map(|g| g.gateway_id.clone())
            })
            .unwrap_or(max_gateway.gateway_id.clone());

        Ok(into_response(&NodeDetailsTemplate {
            node,
            plots: create_plots(pool, node_id, min_time_nanos, offset, &selected_node)
                .await
                .unwrap_or_default(),
            gateway_packet_info,
            selected_node: Some(selected_node),
        }))
    } else {
        Ok(into_response(&NodeDetailsTemplate {
            node,
            plots: Vec::new(),
            gateway_packet_info,
            selected_node: None,
        }))
    }
}

fn plot_labels() -> &'static Vec<(&'static str, &'static str)> {
    static PLOT_LABELS: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    PLOT_LABELS.get_or_init(|| {
        Vec::from([
            ("R", "RX RSSI"),
            ("S", "RX SNR"),
            ("C", "Channel utilization"),
            ("A", "Air util tx"),
            ("V", "Voltage"),
            ("T", "Temperature"),
            ("H", "Relative humidity"),
            ("B", "Barometric pressure"),
        ])
    })
}

async fn create_plots(
    pool: State<SqlitePool>,
    node_id: i64,
    min_time: i64,
    time_zone_offset: FixedOffset,
    gateway_id: &String,
) -> axum::response::Result<Vec<PlotData>> {
    let mut entries_map : HashMap<String, Vec<(DateTime<FixedOffset>, f64)>>= sqlx::query!(
        r#"
            SELECT * FROM (SELECT 'R' as plot_type, rx_time as "time!", CAST(rx_rssi AS REAL) AS "value!" FROM mesh_packets WHERE from_id = ?1 AND rx_time IS NOT NULL AND rx_time > ?2 AND rx_rssi != 0 AND gateway_id = ?3 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'S' as plot_type, rx_time as "time!", rx_snr AS "value!" FROM mesh_packets WHERE from_id = ?1 AND rx_time IS NOT NULL AND rx_time > ?2 AND rx_snr != 0 AND gateway_id = ?3 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'C' as plot_type, time as "time!", channel_utilization AS "value!" FROM device_metrics WHERE node_id = ?1 AND time IS NOT NULL AND time > ?2 AND channel_utilization > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'A' as plot_type, time as "time!", air_util_tx AS "value!" FROM device_metrics WHERE node_id = ?1 AND time IS NOT NULL AND time > ?2 AND air_util_tx > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'V' as plot_type, time as "time!", voltage AS "value!" FROM device_metrics WHERE node_id = ?1 AND time IS NOT NULL AND time > ?2 AND voltage > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'T' as plot_type, time as "time!", temperature AS "value!" FROM environment_metrics WHERE node_id = ?1 AND time IS NOT NULL AND time > ?2 AND temperature > -100 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'H' as plot_type, time as "time!", relative_humidity AS "value!" FROM environment_metrics WHERE node_id = ?1 AND time IS NOT NULL AND time > ?2 AND relative_humidity > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'B' as plot_type, time as "time!", barometric_pressure AS "value!" FROM environment_metrics WHERE node_id = ?1 AND time IS NOT NULL AND time > ?2 AND barometric_pressure > 0 ORDER BY "time!" DESC LIMIT 100)
            ORDER BY plot_type, "time!" DESC;
        "#,
        node_id, min_time, gateway_id
    )
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?
    .into_iter()
    .rev()
    .map(|record| (record.plot_type, (Utc.timestamp_nanos(record.time).with_timezone(&time_zone_offset), record.value)))
    .into_grouping_map()
    .collect();

    Ok(plot_labels()
        .iter()
        .filter_map(|(plot_type, label)| {
            entries_map.remove(*plot_type).and_then(|entries| {
                util::plot::plot_timeseries_svg(label, entries)
                    .ok()
                    .map(|svg| PlotData {
                        label: label.to_string(),
                        svg,
                    })
            })
        })
        .collect_vec())
}

async fn style_json(web_config: State<WebConfig>) -> impl IntoResponse {
    let mut sources = json!({
        "nodes": {
            "type": "geojson",
            "data": { "type": "FeatureCollection", "features": [] },
            "promoteId": "id",
        },
        "neighbors": {
            "type": "geojson",
            "buffer": 512,
            "maxzoom": 10,
            "data": { "type": "FeatureCollection", "features": [] },
        },
        "positions": {
            "type": "geojson",
            "data": { "type": "FeatureCollection", "features": [] },
        },
        "positions-line": {
            "type": "geojson",
            "data": { "type": "FeatureCollection", "features": [] },
        },
    });

    let mut layers: JsonValue = if let Some(pmtiles_url) = &web_config.pmtiles_url {
        sources["protomaps"] = json!({
            "type": "vector",
            "url": pmtiles_url,
            "attribution": "© <a href=\"https://openstreetmap.org\">Openstreetmap</a>"
        });

        let layers = Asset::get("light.json")
            .expect("Cannot find layer style")
            .data;

        serde_json::from_slice(&layers).expect("Cannot parse layer style")
    } else {
        sources["osm"] = json!({
            "type": "raster",
            "tiles": ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
            "tileSize": 256,
            "attribution": "Map tiles by <a target=\"_top\" rel=\"noopener\" href=\"https://tile.openstreetmap.org/\">OpenStreetMap tile servers</a>, under the <a target=\"_top\" rel=\"noopener\" href=\"https://operations.osmfoundation.org/policies/tiles/\">tile usage policy</a>. Data by <a target=\"_top\" rel=\"noopener\" href=\"http://openstreetmap.org\">OpenStreetMap</a>"
        });

        json!([{
            "id": "osm",
            "type": "raster",
            "source": "osm",
        }])
    };

    let layers = layers.as_array_mut().expect("invalid layers object");

    let stellar_layers = vec![
        json!({
            "id": "neighbor-snr",
            "type": "symbol",
            "source": "neighbors",
            "layout": {
                "symbol-placement": "line-center",
                "symbol-avoid-edges": true,
                "symbol-z-order": "source",
                "text-field": ["get", "snr"],
                "text-font": [
                    "Noto Sans Medium",
                ],
                "text-rotation-alignment": "viewport",
                "text-anchor": "center",
                "text-padding": 5,
                "text-size": 16,
                "text-radial-offset": 0.5,
                "text-justify": "auto",
            },
            "paint": {}
        }),
        json!({
                "id": "node-symbols",
                "type": "symbol",
                "source": "nodes",
                "layout": {
                    "symbol-z-order": "source",
                    "text-field": ["get", "display_name"],
                    "text-font": [
                        "Noto Sans Medium",
                    ],
                    "icon-image": "node-symbol",
                    "icon-size": 0.3,
                    "icon-overlap": "never",
                    "icon-optional": true,
                    "text-overlap": "cooperative",
                    "text-radial-offset": 1,
                    "text-size": 18,
                    "text-variable-anchor-offset": ["top", [0, 1], "bottom", [0, -1], "left", [1, 0], "right", [-1, 0]],
                    "text-justify": "auto",
                },
                "paint": {
                    "text-halo-color": "#fff",
                    "text-halo-width": 2,
                    "icon-color": [
                        "interpolate",
                        ["exponential", 2],
                        ["feature-state", "age"],
                        2,
                        "#C00",
                        10,
                        "#000"
                    ],
                    "text-color": [
                        "interpolate",
                        ["exponential", 2],
                        ["feature-state", "age"],
                        2,
                        "#C00",
                        10,
                        "#202"
                    ],
                }
        }),
        json!({

                "id": "neighbor-lines",
                "type": "line",
                "source": "neighbors",
                "paint": {
                    "line-color": "black",
                    "line-opacity": 0.75,
                    "line-dasharray": [5, 5]
                }
        }),
        json!({
                "id": "positions-line",
                "type": "line",
                "source": "positions-line",
                "paint": {
                    "line-color": "red",
                }
        }),
        json!({
                "id": "positions-circle",
                "type": "circle",
                "source": "positions",
                "paint": {
                    "circle-radius": 4,
                    "circle-color": "#FF0000",
                    "circle-stroke-width": 5,
                    "circle-stroke-opacity": 0,
                },
        }),
    ];

    layers.extend_from_slice(&stellar_layers);

    (
        [(header::CONTENT_TYPE, "application/json")],
        json!({
            "version": 8,
            "glyphs": web_config.map_glyphs_url,
            "sources": sources,
            "layers": layers,
        })
        .to_string(),
    )
}

pub async fn start_server(pool: SqlitePool, http_addr: String) -> anyhow::Result<()> {
    let web_config = WebConfig {
        pmtiles_url: get_config().get_string("pmtiles_url").ok(),
        map_glyphs_url: get_config()
            .get_string("map_glyphs_url")
            .expect("Configuration error: map_glyphs_url missing"),
        hide_private_messages: get_config()
            .get_bool("hide_private_messages")
            .unwrap_or(false),
    };
    info!("Starting web server @ {}", http_addr);

    // build our application with a single route
    let app = Router::new()
        .route("/", get(index))
        .route("/events", get(sse_handler))
        .route(
            "/node/:node_id/positions.geojson",
            get(node_positions_geojson),
        )
        .route("/node/:node_id/details.html", get(node_details))
        .route("/map/style.json", get(style_json))
        .route("/static/*file", get(static_handler))
        .fallback_service(get(not_found))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        // Create the application state
        .with_state(AppState { pool, web_config });

    let listener = TcpListener::bind(&http_addr).await?;
    info!("Listening on {}", &http_addr);

    if cfg!(windows)
        && get_config()
            .get_bool("open_browser")
            .expect("Configuration error: open_browser missing")
    {
        tokio::spawn(util::windows::open_browser(http_addr));
    }

    axum::serve(listener, app).await?;
    Ok(())
}

// We use a wildcard matcher ("/dist/*file") to match against everything
// within our defined assets directory. This is the directory on our Asset
// struct below, where folder = "examples/public/".
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let mut path = uri.path().trim_start_matches('/').to_string();

    if path.starts_with("static/") {
        path = path.replace("static/", "");
    }

    StaticFile(path)
}

// Finally, we use a fallback route for anything that didn't match.
async fn not_found() -> Html<&'static str> {
    Html("<h1>404</h1><p>Not Found</p>")
}
