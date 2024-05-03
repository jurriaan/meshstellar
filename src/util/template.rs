use askama::Error;
use askama::Template;
use axum::http::header;
use axum::response::ErrorResponse;
use axum::response::IntoResponse;
use axum::response::Response;

/// Render a [`Template`] into a [`Response`], or render an error page.
pub fn into_response<T: ?Sized + Template>(tmpl: &T) -> Response {
    try_into_response(tmpl)
        .map_err(|err| ErrorResponse::from(err.to_string()))
        .into_response()
}

/// Try to render a [`Template`] into a [`Response`].
pub fn try_into_response<T: ?Sized + Template>(tmpl: &T) -> Result<Response, Error> {
    let value = tmpl.render()?.into();
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static(T::MIME_TYPE),
        )
        .body(value)
        .map_err(|err| Error::Custom(err.into()))
}
