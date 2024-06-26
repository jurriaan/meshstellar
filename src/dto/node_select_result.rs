use sqlx::FromRow;

#[allow(dead_code)]
#[derive(Clone, Debug, FromRow)]
pub struct NodeSelectResult {
    pub node_id: i64,
    pub user_id: String,
    pub last_rx_time: Option<i64>,
    pub last_rx_snr: Option<f64>,
    pub last_rx_rssi: Option<i64>,
    pub last_hop_start: Option<i64>,
    pub last_hop_limit: Option<i64>,
    pub long_name: Option<String>,
    pub short_name: Option<String>,
    pub hw_model_id: Option<i64>,
    pub is_licensed: Option<i64>,
    pub role: Option<i64>,
    pub battery_level: Option<i64>,
    pub voltage: Option<f64>,
    pub channel_utilization: Option<f64>,
    pub air_util_tx: Option<f64>,
    pub uptime_seconds: Option<i64>,
    pub temperature: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub barometric_pressure: Option<f64>,
    pub gas_resistance: Option<f64>,
    pub iaq: Option<i64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<i64>,
    pub neighbor_json: Option<String>,
    pub updated_at: i64,
}
