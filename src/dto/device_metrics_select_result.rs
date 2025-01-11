use sqlx::FromRow;
#[derive(Clone, Debug, FromRow)]
pub struct DeviceMetricsSelectResult {
    pub mesh_packet_id: i64,
    pub time: Option<i64>,
    pub battery_level: Option<i64>,
    pub voltage: Option<f64>,
    pub channel_utilization: Option<f64>,
    pub air_util_tx: Option<f64>,
    pub uptime_seconds: Option<i64>,
}
