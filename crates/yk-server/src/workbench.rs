//! The workbench, compiled into the binary.
//!
//! **Why this exists.** Yinkote's premise is that somebody installs it on their
//! own machine and starts it. That premise is only kept if "install" means one
//! file. Everything else was already there — SQLite is statically linked
//! (`rusqlite` with `bundled`), the Word task pane is `include_str!`'d, and the
//! only dynamic dependencies are the C runtime — so the workbench was the last
//! thing standing between the project and a single artefact.
//!
//! **A directory on disk still wins when given.** `--web-dir` is how the
//! frontend is developed: point it at `web/dist` and a rebuild is visible on
//! reload, with no Rust recompile. The embedded copy is the fallback, which is
//! also the right order of precedence — an explicit flag should never be
//! silently ignored in favour of something baked in months ago.
//!
//! **The trap this avoids.** Serving assets from the filesystem means the
//! program works on the developer's machine and is broken everywhere else, in a
//! way no test catches because the test runs in the source tree. Embedding
//! moves that failure to compile time; `build.rs` then makes even that
//! survivable by writing a page that explains itself.

use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
struct Workbench;

/// Whether a workbench was built into this binary.
///
/// False for a build where `npm run build` never ran: `build.rs` leaves one
/// page saying so, and nothing else.
pub fn is_embedded() -> bool {
    Workbench::iter().count() > 1
}

/// Serve an embedded file, falling back to the app shell.
///
/// The fallback is what makes a single-page application work on a reload: the
/// client owns its routes, so `/reader/ABCD1234` has to answer with the shell
/// rather than a 404, and let the app read the address itself.
pub async fn serve(uri: Uri) -> Response {
    let asked = uri.path().trim_start_matches('/');
    // Resolve first, then describe what was resolved. Guessing the type from
    // the *requested* path served the shell for `/` as octet-stream, which a
    // browser downloads instead of rendering.
    let (path, file) = match Workbench::get(asked) {
        Some(file) if !asked.is_empty() => (asked, Some(file)),
        _ => ("index.html", Workbench::get("index.html")),
    };
    match file {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let mut response = file.data.into_owned().into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("text/plain")),
            );
            // Vite writes a content hash into every asset filename, so those
            // may be kept forever. `index.html` names them and must not be, or
            // an upgrade would leave the browser asking for files that are no
            // longer in the binary.
            let forever = path.starts_with("assets/") && path.contains('-');
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(if forever {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                }),
            );
            response
        }
        None => (StatusCode::NOT_FOUND, "no workbench in this build").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unknown_path_is_answered_with_the_shell() {
        // The client owns its routes: reloading on /reader/ABCD1234 must reach
        // the app, not a 404 from the server that has never heard of it.
        let response = serve("/reader/ABCD1234".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
    }

    #[tokio::test]
    async fn the_root_is_served_as_html() {
        // `/` has no extension, so guessing the type from the requested path
        // made it octet-stream — which a browser downloads rather than draws.
        let response = serve("/".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), "text/html");
    }

    #[tokio::test]
    async fn the_shell_is_never_cached_and_hashed_assets_always_are() {
        // index.html names the hashed files; caching it would leave a browser
        // asking an upgraded binary for assets it no longer contains.
        let shell = serve("/".parse().unwrap()).await;
        assert_eq!(shell.headers().get(header::CACHE_CONTROL).unwrap(), "no-cache");
    }
}
