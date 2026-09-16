//! The interface bundle, served from inside the binary.
//!
//! The same `ui/dist` the desktop shell embeds is embedded here, so a phone
//! gets exactly the interface the desktop shows. Anything that is not a file
//! in the bundle gets `index.html`: the interface is a single page and keeps
//! its own state in the URL fragment.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../ui/dist"]
struct Ui;

pub(crate) async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Ui::get(path) {
        Some(file) => respond(path, file),
        // A path with an extension is a file that is genuinely missing; a bare
        // route is the interface's to handle.
        None if path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.')) =>
        {
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
        None => match Ui::get("index.html") {
            Some(file) => respond("index.html", file),
            None => (
                StatusCode::NOT_FOUND,
                "The interface bundle is missing from this build.",
            )
                .into_response(),
        },
    }
}

fn respond(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = file.metadata.mimetype().to_string();
    // Vite names assets by content hash, so they can be cached forever; the
    // page that refers to them cannot be.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        file.data.into_owned(),
    )
        .into_response()
}
