use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct ReturningId {
    pub id: i64,
}
