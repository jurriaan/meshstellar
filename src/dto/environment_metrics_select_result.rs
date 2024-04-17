use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct EnvironmentMetricsSelectResult {
    pub mesh_packet_id: i64,
    pub time: Option<i64>,
    pub temperature: f64,
    pub relative_humidity: f64,
    pub barometric_pressure: f64,
    pub gas_resistance: f64,
    pub iaq: i64,
}
