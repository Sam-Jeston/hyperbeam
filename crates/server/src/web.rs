//! The web UI, embedded in the binary.

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // API-shaped paths that matched no route are errors, not pages.
    if ["api/", "git/", "mcp", "runner/"].iter().any(|p| path.starts_with(p)) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (file, name) = match Assets::get(path) {
        Some(file) if !path.is_empty() => (Some(file), path),
        // Client-side routes all load the app shell.
        _ => (Assets::get("index.html"), "index.html"),
    };
    match file {
        Some(file) => {
            let mime = mime_guess::from_path(name).first_or_octet_stream();
            // Bundles are content-hashed by the bundler; the shell is not.
            let cache = if name == "index.html" { "no-cache" } else { "public, max-age=31536000, immutable" };
            ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], file.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "The web UI was not built into this binary. Run `bun run build` in web/ and rebuild.").into_response(),
    }
}
