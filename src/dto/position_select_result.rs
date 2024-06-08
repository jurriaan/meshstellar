use sqlx::FromRow;

#[derive(Clone, Debug, FromRow, Copy)]
pub struct PositionSelectResult {
    pub mesh_packet_id: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: i64,
    pub sats_in_view: i64,
    pub precision_bits: i64,
}
