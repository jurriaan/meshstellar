use sqlx::FromRow;

#[derive(Clone, Debug, FromRow, Copy)]
pub struct PositionSelectResult {
    pub mesh_packet_id: i64,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<i64>,
    pub sats_in_view: Option<i64>,
    pub precision_bits: Option<i64>,
    pub ground_speed: Option<i64>,
    pub seq_number: Option<i64>,
    pub ground_track: Option<i64>,
}
