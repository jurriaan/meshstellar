use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct WaypointSelectResult {
    pub mesh_packet_id: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub expire: Option<i64>,
    pub locked_to: Option<i64>,
    pub name: String,
    pub description: String,
    pub icon: String,
}
