use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct EnvironmentMetricsSelectResult {
    pub mesh_packet_id: i64,
    pub time: Option<i64>,
    pub temperature: Option<f64>,
    pub relative_humidity: Option<f64>,
    pub barometric_pressure: Option<f64>,
    pub gas_resistance: Option<f64>,
    pub iaq: Option<i64>,
}
