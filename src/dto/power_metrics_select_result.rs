use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct PowerMetricsSelectResult {
    pub mesh_packet_id: i64,
    pub time: Option<i64>,
    pub ch1_voltage: f64,
    pub ch1_current: f64,
    pub ch2_voltage: f64,
    pub ch2_current: f64,
    pub ch3_voltage: f64,
    pub ch3_current: f64,
}
