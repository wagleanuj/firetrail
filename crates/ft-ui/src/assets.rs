//! Static asset serving.
//!
//! When built with `--features bundled-ui`, files under `crates/ft-ui/web/dist/`
//! are embedded into the binary via `rust_embed`. Without the feature, every
//! asset request returns a helpful 404 explaining how to enable bundling or how
//! to run in `--dev` mode where Vite serves assets.
//!
//! **Design decision (Wave 0):** `bundled-ui` is a *hard requirement* on the
//! `web/dist/` directory. Building `--features bundled-ui` before the
//! web-scaffold subagent lands `web/dist/` will fail at compile time. We chose
//! this over the `has_web_dist` build.rs probe because (a) it keeps Wave 0
//! plumbing minimal, (b) the failure is loud and actionable, and (c) the
//! default build (no bundled-ui) compiles and runs cleanly — useful for
//! `cargo check` and headless tests.

use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
#[cfg(feature = "bundled-ui")]
use axum::http::header;

/// Serve `index.html` for the SPA root after auth bootstrap.
pub fn serve_index() -> Response {
    #[cfg(feature = "bundled-ui")]
    {
        if let Some(file) = Assets::get("index.html") {
            return (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                file.data.into_owned(),
            )
                .into_response();
        }
    }
    not_bundled_response("index.html")
}

/// Serve an arbitrary asset path (e.g. `assets/index-abc.js`).
pub async fn serve(Path(path): Path<String>) -> Response {
    #[cfg(feature = "bundled-ui")]
    {
        let lookup = format!("assets/{path}");
        if let Some(file) = Assets::get(&lookup) {
            let mime = mime_from_path(&lookup);
            return ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response();
        }
        return not_bundled_response(&lookup);
    }
    #[cfg(not(feature = "bundled-ui"))]
    {
        not_bundled_response(&path)
    }
}

fn not_bundled_response(what: &str) -> Response {
    let body = format!(
        "ft-ui: asset `{what}` not available.\n\nRebuild with `--features bundled-ui` to embed \
         the SPA, or run the server in `--dev` mode and let Vite serve assets from :5173.\n"
    );
    (StatusCode::NOT_FOUND, body).into_response()
}

#[cfg(feature = "bundled-ui")]
fn mime_from_path(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(feature = "bundled-ui")]
#[derive(rust_embed::Embed)]
#[folder = "web/dist/"]
struct Assets;
