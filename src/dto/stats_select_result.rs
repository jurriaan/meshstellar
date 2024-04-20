use sqlx::FromRow;

#[derive(Clone, Debug, FromRow, Default)]
pub struct StatsSelectResult {
    pub num_packets: i64,
    pub num_nodes: i64,
}
