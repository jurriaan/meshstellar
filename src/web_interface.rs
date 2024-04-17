use crate::{
    dto::{
        mesh_packet::Payload, DeviceMetricsSelectResult, EnvironmentMetricsSelectResult,
        MeshPacket as MeshPacketDto, NeighborSelectResult, NodeSelectResult, PlotData,
        PositionSelectResult, WaypointSelectResult,
    },
    proto::meshtastic::PortNum,
    template::*,
    util::{
        self,
        config::get_config,
        demoji,
        static_file::{Asset, StaticFile},
        stringify, DatabaseError,
    },
};
use askama::Template;
use async_stream::stream;
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
use chrono::{DateTime, TimeZone, Utc};
use futures::{
    stream::{select_all, Stream},
    FutureExt,
};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, JsonObject, JsonValue};
use itertools::Itertools;
use serde_json::json;
use sqlx::SqlitePool;
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
}

// Define your application shared state
#[derive(Clone, FromRef)]
struct AppState {
    pool: SqlitePool,
    web_config: WebConfig,
}

async fn index() -> impl IntoResponse {
    IndexTemplate {}
}

fn node_update_stream(
    pool: State<SqlitePool>,
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    let mut last_rx_time: i64 = 0;

    Box::pin(stream! {
        loop {
            let nodes = sqlx::query_as!(
                NodeSelectResult,
                r#"
                SELECT 
                    node_id,
                    user_id,
                    last_rx_time,
                    last_rx_snr,
                    last_rx_rssi,
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
                    last_node_info_id,
                    last_position_id,
                    last_device_metrics_id,
                    last_environment_metrics_id
                FROM nodes
                WHERE last_rx_time > ?
                ORDER BY last_rx_time ASC
                "#,
                last_rx_time
            )
            .fetch_all(&*pool)
            .await;

            if let Err(err) = nodes {
                error!("Error occurred while fetching nodes: {}", err);
                break;
            }

            let nodes = nodes.unwrap_or_default();

            last_rx_time = nodes.last().and_then(|n| n.last_rx_time).unwrap_or(last_rx_time);

            for node in nodes.into_iter() {
                let geom = if let (Some(longitude), Some(latitude), Some(altitude)) = (node.longitude, node.latitude, node.altitude) {
                    let geometry: Geometry = geojson::Value::Point(vec![longitude, latitude, altitude as f64]).into();
                    let mut properties = JsonObject::new();
                    let short_name = demoji(node.short_name.clone().unwrap_or_default().as_str());
                    let display_name = match short_name.trim() {
                        "" => node.user_id.clone(),
                        _ => short_name
                    };

                    properties.insert("display_name".to_string(), JsonValue::from(display_name));
                    properties.insert("long_name".to_string(), JsonValue::from(node.long_name.clone()));
                    properties.insert("last_rx_time".to_string(), JsonValue::from(node.last_rx_time.unwrap_or_default() as f64 / 1_000_000_000.0));

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
) -> Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> {
    let mut last_rx_time: i64 = 0;

    Box::pin(stream! {
        // Delay by half a second to allow nodes to load first
        tokio::time::sleep(Duration::from_millis(500)).await;

        loop {
            let packets : Result<Vec<MeshPacketDto>, sqlx::Error> = sqlx::query_as(
                r#"
                SELECT *
                FROM mesh_packets
                WHERE id IN (
                    SELECT id FROM mesh_packets
                    WHERE rx_time > ?1
                    ORDER BY rx_time DESC
                    LIMIT 100
                ) OR id IN (
                    SELECT id FROM mesh_packets
                    WHERE rx_time > ?1 AND portnum = ?2
                    ORDER BY rx_time DESC
                    LIMIT 100
                )
                ORDER BY rx_time DESC
                "#,
            )
            .bind(last_rx_time)
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
                let query = format!("SELECT * FROM waypoints WHERE mesh_packet_id IN ({})", packet_ids_string);
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
                let query = format!("SELECT * FROM positions WHERE mesh_packet_id IN ({})", packet_ids_string);
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
                let query = format!("SELECT * FROM device_metrics WHERE mesh_packet_id IN ({})", packet_ids_string);
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
                let query = format!("SELECT * FROM enviroment_metrics WHERE mesh_packet_id IN ({})", packet_ids_string);
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
            let compute_neighbors = || {
                async {
                    let query = format!("SELECT * FROM neighbors WHERE mesh_packet_id IN ({}) ORDER BY mesh_packet_id, id", packet_ids_string);
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

            let waypoints : OnceCell<HashMap<i64, WaypointSelectResult>> = OnceCell::new();
            let positions : OnceCell<HashMap<i64, PositionSelectResult>> = OnceCell::new();
            let device_metrics : OnceCell<HashMap<i64, DeviceMetricsSelectResult>> = OnceCell::new();
            let environment_metrics: OnceCell<HashMap<i64, EnvironmentMetricsSelectResult>> = OnceCell::new();
            let neighbors: OnceCell<HashMap<i64, Vec<NeighborSelectResult>>> = OnceCell::new();

            last_rx_time = packets.first().map(|p| p.rx_time).unwrap_or(last_rx_time);

            for mut packet in packets.into_iter().rev() {
                let mut event_type = "mesh-packet";
                match PortNum::try_from(packet.portnum) {
                    Ok(PortNum::TextMessageApp) => {
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
                            packet.payload = Payload::Position(position.clone())
                        }
                    }
                    Ok(PortNum::TelemetryApp) => {
                        if let Some(device_metrics) = device_metrics.get_or_init(compute_device_metrics).await.get(&packet.id) {
                            packet.payload = Payload::DeviceMetrics(device_metrics.clone())
                        } else if let Some(environment_metrics) = environment_metrics.get_or_init(compute_environment_metrics).await.get(&packet.id) {
                            packet.payload = Payload::EnvironmentMetrics(environment_metrics.clone())
                        }
                    }
                    Ok(PortNum::NeighborinfoApp) => {
                        if let Some(neighbors) = neighbors.get_or_init(compute_neighbors).await.get(&packet.id) {
                            packet.payload = Payload::Neighbors(neighbors.clone());
                        }
                    }
                    _ => {}
                }
                let template = PacketTemplate { packet };

                if let Ok(data) = template.render() {
                    yield Ok(Event::default().event(event_type).data(data))
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

async fn sse_handler(
    pool: State<SqlitePool>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let update_stream =
        select_all(vec![node_update_stream(pool.clone()), mesh_packet_stream(pool)].into_iter());

    Sse::new(update_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("alive"),
    )
}

struct NodePosition {
    latitude: f64,
    longitude: f64,
    altitude: i64,
}

impl FromIterator<NodePosition> for Feature {
    fn from_iter<T: IntoIterator<Item = NodePosition>>(iter: T) -> Self {
        let line_string = iter
            .into_iter()
            .map(|pos| vec![pos.longitude, pos.latitude, pos.altitude as f64].into())
            .collect_vec();

        let geometry: Geometry = geojson::Value::LineString(line_string).into();

        Feature {
            geometry: Some(geometry),
            ..Default::default()
        }
    }
}

async fn node_positions_geojson(
    pool: State<SqlitePool>,
    Path((node_id,)): Path<(String,)>,
) -> axum::response::Result<impl IntoResponse> {
    let node_id = u32::from_str_radix(&node_id, 16).map_err(stringify)?;
    let query_res: Vec<NodePosition> = sqlx::query_as!(
        NodePosition,
        r#"
            SELECT 
                positions.latitude,
                positions.longitude,
                positions.altitude
            FROM positions 
            JOIN mesh_packets ON positions.mesh_packet_id = mesh_packets.id 
            WHERE node_id = ?
            ORDER BY rx_time DESC
            LIMIT 250
            "#,
        node_id,
    )
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?;

    let res = GeoJson::Feature(query_res.into_iter().collect()).to_string();

    Ok(([(header::CONTENT_TYPE, "application/geo+json")], res))
}

#[derive(sqlx::FromRow)]
struct NeighborLine {
    node_a_longitude: f64,
    node_a_latitude: f64,
    node_b_longitude: f64,
    node_b_latitude: f64,
    min_snr: f64,
}

impl FromIterator<NeighborLine> for FeatureCollection {
    fn from_iter<T: IntoIterator<Item = NeighborLine>>(iter: T) -> Self {
        iter.into_iter()
            .map(|line| {
                let line_string = vec![
                    vec![line.node_a_longitude, line.node_a_latitude],
                    vec![line.node_b_longitude, line.node_b_latitude],
                ];
                let geometry: Geometry = geojson::Value::LineString(line_string).into();

                let mut properties = JsonObject::new();
                properties.insert("min_snr".to_string(), JsonValue::from(line.min_snr));

                Feature {
                    geometry: Some(geometry),
                    properties: Some(properties),
                    ..Default::default()
                }
            })
            .collect()
    }
}

async fn node_neighbors_geojson(
    pool: State<SqlitePool>,
    Path((node_id,)): Path<(String,)>,
) -> axum::response::Result<impl IntoResponse> {
    let node_id = i64::from_str_radix(&node_id, 16).map_err(stringify)? as i64;

    let query_res: Vec<NeighborLine> = sqlx::query_as!(
        NeighborLine,
        r#"
            WITH source_data AS 
            (
            SELECT
                node_id as node_a,
                neighbor_node_id as node_b,
                snr as min_snr
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
                        AND node_id = ?
                )
                a 
            WHERE
                row_number = 1 
            )
            SELECT
                nodes_a.longitude AS "node_a_longitude!",
                nodes_a.latitude AS "node_a_latitude!",
                nodes_b.longitude AS "node_b_longitude!",
                nodes_b.latitude AS "node_b_latitude!",
                min_snr 
            FROM
                source_data 
                LEFT JOIN
                    nodes nodes_a 
                    ON nodes_a.node_id = node_a 
                LEFT JOIN
                    nodes nodes_b 
                    ON nodes_b.node_id = node_b 
            WHERE
                nodes_a.longitude IS NOT NULL 
                AND nodes_a.latitude IS NOT NULL
                AND nodes_b.longitude IS NOT NULL
                AND nodes_b.latitude IS NOT NULL
            ORDER BY min_snr DESC
            ;
            "#,
        node_id
    )
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?;

    Ok((
        [(header::CONTENT_TYPE, "application/geo+json")],
        query_res
            .into_iter()
            .collect::<FeatureCollection>()
            .to_string(),
    ))
}

async fn neighbors_geojson(pool: State<SqlitePool>) -> axum::response::Result<impl IntoResponse> {
    let query_res: Vec<NeighborLine> = sqlx::query_as!(
        NeighborLine,
        r#"
            WITH source_data AS 
            (
            SELECT
                MIN(node_id, neighbor_node_id) as node_a,
                MAX(node_id, neighbor_node_id) as node_b,
                MIN(snr) as min_snr 
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
                        timestamp > (strftime('%s', 'now') * 1000000000) - (4 * 60 * 60 * 1000000000)
                )
                a 
            WHERE
                row_number = 1 
            GROUP BY
                node_a,
                node_b
            )
            SELECT
                nodes_a.longitude AS "node_a_longitude!",
                nodes_a.latitude AS "node_a_latitude!",
                nodes_b.longitude AS "node_b_longitude!",
                nodes_b.latitude AS "node_b_latitude!",
                min_snr 
            FROM
                source_data 
                JOIN
                    nodes nodes_a 
                    ON nodes_a.node_id = node_a 
                JOIN
                    nodes nodes_b 
                    ON nodes_b.node_id = node_b 
            WHERE
                nodes_a.longitude IS NOT NULL 
                AND nodes_a.latitude IS NOT NULL
                AND nodes_b.longitude IS NOT NULL
                AND nodes_b.latitude IS NOT NULL
            ORDER BY min_snr DESC
            ;
            "#
    )
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?;

    Ok((
        [(header::CONTENT_TYPE, "application/geo+json")],
        query_res
            .into_iter()
            .collect::<FeatureCollection>()
            .to_string(),
    ))
}

async fn node_details(
    pool: State<SqlitePool>,
    Path((node_id,)): Path<(String,)>,
) -> axum::response::Result<impl IntoResponse> {
    let node_id = i64::from_str_radix(&node_id, 16).map_err(stringify)? as i64;
    let node = sqlx::query_as!(
        NodeSelectResult,
        r#"
        SELECT 
            node_id,
            user_id,
            last_rx_time,
            last_rx_snr,
            last_rx_rssi,
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
            last_node_info_id,
            last_position_id,
            last_device_metrics_id,
            last_environment_metrics_id
        FROM nodes
        WHERE node_id = ?
        ORDER BY last_rx_time ASC
        "#,
        node_id
    )
    .fetch_one(&*pool)
    .await
    .map_err(DatabaseError)?;

    Ok(NodeDetailsTemplate {
        node,
        plots: create_plots(pool, node_id).await.unwrap_or_default(),
    })
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
) -> axum::response::Result<Vec<PlotData>> {
    let mut entries_map : HashMap<String, Vec<(DateTime<Utc>, f64)>>= sqlx::query!(
        r#"
            SELECT * FROM (SELECT 'R' as plot_type, rx_time as "time!", CAST(rx_rssi AS REAL) AS "value!" FROM mesh_packets WHERE from_id = ?1 AND rx_time IS NOT NULL AND rx_rssi != 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'S' as plot_type, rx_time as "time!", rx_snr AS "value!" FROM mesh_packets WHERE from_id = ?1 AND rx_time IS NOT NULL AND rx_snr != 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'C' as plot_type, time as "time!", channel_utilization AS "value!" FROM device_metrics WHERE node_id = ?1 AND time IS NOT NULL AND channel_utilization > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'A' as plot_type, time as "time!", air_util_tx AS "value!" FROM device_metrics WHERE node_id = ?1 AND time IS NOT NULL AND air_util_tx > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'V' as plot_type, time as "time!", voltage AS "value!" FROM device_metrics WHERE node_id = ?1 AND time IS NOT NULL AND voltage > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'T' as plot_type, time as "time!", temperature AS "value!" FROM environment_metrics WHERE node_id = ?1 AND time IS NOT NULL AND temperature > -100 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'H' as plot_type, time as "time!", relative_humidity AS "value!" FROM environment_metrics WHERE node_id = ?1 AND time IS NOT NULL AND relative_humidity > 0 ORDER BY "time!" DESC LIMIT 100)
            UNION ALL SELECT * FROM (SELECT 'B' as plot_type, time as "time!", barometric_pressure AS "value!" FROM environment_metrics WHERE node_id = ?1 AND time IS NOT NULL AND barometric_pressure > 0 ORDER BY "time!" DESC LIMIT 100) 
            ORDER BY plot_type, "time!" DESC; 
        "#,
        node_id
    )
    .fetch_all(&*pool)
    .await
    .map_err(DatabaseError)?
    .into_iter()
    .rev()
    .map(|record| (record.plot_type, (Utc.timestamp_nanos(record.time), record.value)))
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
        },
        "neighbors": {
            "type": "geojson",
            "data": "/neighbors.geojson",
        },
        "positions": {
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

        serde_json::from_slice(&*layers).expect("Cannot parse layer style")
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
                    "icon-size": 0.2,
                    "icon-overlap": "never",
                    "icon-optional": true,
                    "text-overlap": "cooperative",
                    "text-radial-offset": 1,
                    "text-size": 18,
                    "text-variable-anchor": ["left", "right", "top", "bottom"],
                    "text-justify": "auto",
                },
                "paint": {
                    "text-color": "#202",
                    "text-halo-color": "#fff",
                    "text-halo-width": 2
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
                "source": "positions",
                "paint": {
                    "line-color": "red",
                }
        }),
        json!({
            "id": "neighbor-snr",
            "type": "symbol",
            "source": "neighbors",
            "layout": {
                "symbol-placement": "line-center",
                "symbol-avoid-edges": true,
                "symbol-z-order": "source",
                "text-field": ["get", "min_snr"],
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
    };
    info!("Starting web server @ {}", http_addr);

    // build our application with a single route
    let app = Router::new()
        .route("/", get(index))
        .route("/events", get(sse_handler))
        .route("/neighbors.geojson", get(neighbors_geojson))
        .route(
            "/node/:node_id/positions.geojson",
            get(node_positions_geojson),
        )
        .route(
            "/node/:node_id/neighbors.geojson",
            get(node_neighbors_geojson),
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
