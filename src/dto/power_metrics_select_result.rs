use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct PowerMetricsSelectResult {
    pub mesh_packet_id: i64,
    pub time: Option<i64>,
    pub ch1_voltage: Option<f64>,
    pub ch1_current: Option<f64>,
    pub ch2_voltage: Option<f64>,
    pub ch2_current: Option<f64>,
    pub ch3_voltage: Option<f64>,
    pub ch3_current: Option<f64>,
}
