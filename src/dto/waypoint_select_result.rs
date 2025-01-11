use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct WaypointSelectResult {
    pub mesh_packet_id: i64,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub expire: Option<i64>,
    pub locked_to: Option<i64>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
}
