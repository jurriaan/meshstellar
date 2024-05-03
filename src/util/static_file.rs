use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::response::Response;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$PROCESSED_STATIC_PATH/"]
pub struct Asset;

pub struct StaticFile<T>(pub T);

impl<T> IntoResponse for StaticFile<T>
where
    T: Into<String>,
{
    fn into_response(self) -> Response {
        let path: String = self.0.into();

        match Asset::get(path.as_str()) {
            Some(content) => {
                if path.ends_with(".gz") {
                    let path_without_gz = path[..path.len() - 3].to_string();

                    let mime = mime_guess::from_path(path_without_gz).first_or_octet_stream();

                    (
                        [
                            (header::CONTENT_TYPE, mime.as_ref()),
                            (header::CONTENT_ENCODING, "gzip"),
                        ],
                        content.data,
                    )
                        .into_response()
                } else {
                    let mime = mime_guess::from_path(path).first_or_octet_stream();
                    ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
                }
            }
            None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
        }
    }
}
