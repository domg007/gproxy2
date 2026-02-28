use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct AdminUiAssets;

pub async fn index() -> Response {
    render("index.html")
}

pub async fn asset(Path(path): Path<String>) -> Response {
    render(format!("assets/{path}").as_str())
}

fn render(path: &str) -> Response {
    let Some(content) = AdminUiAssets::get(path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    let mime = mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream");

    let mut response = Response::new(axum::body::Body::from(content.data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response
}
