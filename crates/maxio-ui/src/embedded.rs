use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../ui/build"]
struct UiAssets;

pub async fn ui_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "200.html" } else { path };
    let requested_file = UiAssets::get(path);
    let missing_asset = requested_file.is_none()
        && path
            .rsplit('/')
            .next()
            .is_some_and(|segment| segment.contains('.'));

    match requested_file.map(|file| (file, path)).or_else(|| {
        (!missing_asset)
            .then(|| UiAssets::get("200.html").map(|file| (file, "200.html")))
            .flatten()
    }) {
        Some((file, served_path)) => {
            let mime = mime_guess::from_path(served_path).first_or_octet_stream();
            let hash = file.metadata.sha256_hash();
            let etag = hex::encode(&hash[..8]);
            let is_shell = served_path == "200.html" || served_path.ends_with(".html");
            let cache_control = if is_shell {
                "no-store, must-revalidate"
            } else {
                "public, max-age=31536000, immutable"
            };

            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    (header::ETAG, format!("\"{etag}\"")),
                    (header::CACHE_CONTROL, cache_control.to_string()),
                ],
                file.data,
            )
                .into_response()
        }
        None if missing_asset => (StatusCode::NOT_FOUND, "Asset not found").into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "UI not built. Run: cd ui && bun run build",
        )
            .into_response(),
    }
}
