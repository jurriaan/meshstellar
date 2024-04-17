use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;

pub struct DatabaseError(pub sqlx::Error);

impl IntoResponse for DatabaseError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database Error: {:?}", self.0),
        )
            .into_response()
    }
}
