use sqlx::FromRow;
#[derive(Clone, Debug, FromRow)]
pub struct DeviceMetricsSelectResult {
    pub mesh_packet_id: i64,
    pub time: Option<i64>,
    pub battery_level: i64,
    pub voltage: f64,
    pub channel_utilization: f64,
    pub air_util_tx: f64,
    pub uptime_seconds: i64,
}
