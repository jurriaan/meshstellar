use sqlx::sqlite::SqliteRow;
use sqlx::FromRow;
use sqlx::Row;

use crate::proto::meshtastic::{mesh_packet::Priority, PortNum};
use crate::util::capitalize;

use super::{
    DeviceMetricsSelectResult, EnvironmentMetricsSelectResult, NeighborSelectResult,
    PositionSelectResult, PowerMetricsSelectResult, RoutingDto, TracerouteDto,
    WaypointSelectResult,
};

#[derive(Clone, Default)]
pub enum Payload {
    TextMessage(String),
    Waypoint(WaypointSelectResult),
    Position(PositionSelectResult),
    DeviceMetrics(DeviceMetricsSelectResult),
    EnvironmentMetrics(EnvironmentMetricsSelectResult),
    PowerMetrics(PowerMetricsSelectResult),
    Neighbors(Vec<NeighborSelectResult>),
    Traceroute(TracerouteDto),
    Routing(RoutingDto),
    #[default]
    Unknown,
}

#[derive(Clone)]
pub struct MeshPacket {
    pub id: i64,
    pub from_id: u32,
    pub to_id: u32,
    pub gateway_id: Option<u32>,
    pub portnum: i32,
    pub packet_type: String,
    pub rx_time: i64,
    pub hop_start: Option<u8>,
    pub num_hops: Option<u8>,
    pub rx_snr: f64,
    pub rx_rssi: i64,
    pub priority: String,
    pub want_ack: bool,
    pub want_response: bool,
    pub payload: Payload,
    pub payload_data: Vec<u8>,
    pub created_at: i64,
    pub received_at: i64,
}

fn parse_hexadecimal_id(input: &str) -> Option<u32> {
    input
        .strip_prefix('!')
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
}

impl FromRow<'_, SqliteRow> for MeshPacket {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<i64, _>("id").unwrap_or_default();
        let from_id = row.try_get::<i64, _>("from_id").unwrap_or_default();
        let to_id = row.try_get::<i64, _>("to_id").unwrap_or_default();
        let gateway_id = row.try_get::<String, _>("gateway_id").unwrap_or_default();
        let portnum = row.try_get::<i64, _>("portnum").unwrap_or_default();
        let rx_time = row.try_get::<i64, _>("rx_time").unwrap_or_default();
        let hop_start = row.try_get::<i64, _>("hop_start").unwrap_or_default();
        let hop_limit = row.try_get::<i64, _>("hop_limit").unwrap_or_default();
        let rx_snr = row.try_get::<f64, _>("rx_snr").unwrap_or_default();
        let rx_rssi = row.try_get::<i64, _>("rx_rssi").unwrap_or_default();
        let priority = row.try_get::<i64, _>("priority").unwrap_or_default();
        let want_ack = row.try_get::<i64, _>("want_ack").unwrap_or_default();
        let want_response = row.try_get::<i64, _>("want_response").unwrap_or_default();
        let payload_data = row
            .try_get::<Vec<u8>, _>("payload_data")
            .unwrap_or_default();
        let created_at = row.try_get::<i64, _>("created_at").unwrap_or_default();
        let received_at = row.try_get::<i64, _>("received_at").unwrap_or_default();

        let (hop_start, num_hops) = if hop_start >= hop_limit && hop_start != 0 {
            (Some(hop_start as u8), Some((hop_start - hop_limit) as u8))
        } else {
            (None, None)
        };

        let priority_string = Priority::try_from(priority as i32)
            .map(|p| p.as_str_name().to_lowercase())
            .unwrap_or_else(|_| "unknown".to_string());

        let packet_type = PortNum::try_from(portnum as i32)
            .map(|p| {
                capitalize(
                    p.as_str_name()
                        .replace("_APP", "")
                        .replace('_', " ")
                        .as_str(),
                )
            })
            .unwrap_or_else(|_| "Unknown".to_string());

        Ok(MeshPacket {
            id,
            from_id: from_id as u32,
            to_id: to_id as u32,
            gateway_id: parse_hexadecimal_id(gateway_id.as_str()),
            portnum: portnum as i32,
            packet_type,
            rx_time,
            hop_start,
            num_hops,
            rx_snr,
            rx_rssi,
            priority: priority_string,
            want_ack: want_ack != 0,
            want_response: want_response != 0,
            payload_data,
            payload: Payload::Unknown,
            created_at,
            received_at,
        })
    }
}
