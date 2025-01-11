use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct NeighborSelectResult {
    pub mesh_packet_id: i64,
    pub neighbor_node_id: i64,
    pub snr: Option<f64>,
}
