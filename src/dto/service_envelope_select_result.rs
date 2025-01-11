use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct ServiceEnvelopeSelectResult {
    pub id: i64,
    pub hash: Vec<u8>,
    pub payload_data: Vec<u8>,
    pub created_at: i64,
}
