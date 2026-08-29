//! Page thumbnails.
//!
//! **Why the browser renders these and the server only keeps them.** Yinkote
//! has no rasteriser and should not grow one: rendering a PDF page in Rust
//! means pdfium or mupdf — a large native dependency, built per platform, for a
//! program whose whole premise is that a user can install it. Meanwhile the
//! workbench already has pdf.js and has already drawn the page. Asking the
//! client to hand back what it just drew costs nothing and adds no dependency.
//!
//! So this is a cache with an HTTP face, not an image service:
//!
//! - `GET` answers with the cached image, or **404 when there is none**. The
//!   404 is the protocol: it is how the client learns it should render one.
//! - `PUT` stores what the client rendered.
//!
//! Everything here is derived. `cache/` may be deleted at any moment and the
//! only cost is redrawing, which is why it is not `storage/`.
//!
//! Two limits keep the cache a cache. Widths come from a fixed set, so a
//! caller cannot mint ten thousand variants of one page by counting upwards;
//! and bytes are checked to actually be an image, because "the client sends
//! its own file for us to store and serve back" is otherwise a way to host
//! arbitrary content on the user's own origin.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path as FsPath, PathBuf};
use yk_core::{Error, Key, Result};

use super::key;
use crate::error::ApiResult;
use crate::state::App;

/// Widths a thumbnail may be asked for.
///
/// A fixed set rather than a range: every distinct width is a separate file on
/// disk, and a range lets one caller fill the cache with 240px, 241px, 242px…
/// These are the sizes the workbench actually draws — a row glyph, a card
/// cover, and a reader sidebar page.
const WIDTHS: [u32; 3] = [96, 240, 480];

/// Generous for a thumbnail, small enough that the cache cannot be used as
/// storage. A 480px page as PNG is tens of kilobytes.
const MAX_BYTES: usize = 2 * 1024 * 1024;

pub fn router() -> Router<App> {
    Router::new()
        .route("/libraries/:lib/items/:key/thumbnail", get(fetch).put(store))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    #[serde(default = "first_page")]
    page: u32,
    #[serde(default = "default_width", rename = "w")]
    width: u32,
}

fn first_page() -> u32 {
    1
}

fn default_width() -> u32 {
    240
}

/// The cache filename for one page at one width.
///
/// Flat rather than nested, and built only from values already validated — a
/// key is `[A-Z0-9]{8}` and the numbers are bounded — so no part of it can come
/// from a caller's string.
fn cache_name(key: &Key, page: u32, width: u32, ext: &str) -> String {
    format!("{key}-p{page}-w{width}.{ext}")
}

/// Check the request and say where the file would live.
///
/// One function for both verbs: `GET` and `PUT` have to agree about the
/// filename, and the way they stop agreeing is by each computing it.
fn locate(cache: &FsPath, key: &Key, params: &Params, ext: &str) -> Result<PathBuf> {
    if params.page == 0 || params.page > 10_000 {
        return Err(Error::invalid("page is out of range"));
    }
    if !WIDTHS.contains(&params.width) {
        return Err(Error::invalid(format!(
            "width must be one of {WIDTHS:?}; every distinct width is a file on disk"
        )));
    }
    Ok(cache.join(cache_name(key, params.page, params.width, ext)))
}

/// The one directory cached pages live in.
fn cache_dir(app: &App) -> PathBuf {
    app.config().cache_dir().join("thumbnails")
}

async fn fetch(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Query(params): Query<Params>,
) -> ApiResult<Response> {
    let key = key(&k)?;
    for (ext, mime) in KINDS {
        let path = locate(&cache_dir(&app), &key, &params, ext)?;
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
            // Immutable: the name already contains everything that identifies
            // the image, so a changed page means a changed name.
            headers.insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("private, max-age=604800, immutable"),
            );
            return Ok((headers, bytes).into_response());
        }
    }

    // Not an error the user should see — it is how the client is told to
    // render one and PUT it back. Checking the item exists keeps the 404
    // honest: "no thumbnail yet" and "no such item" are different answers.
    app.store().items.get(lib, &key).await?;
    Err(Error::not_found("no thumbnail cached for this page").into())
}

/// Image kinds accepted, in the order `fetch` looks for them.
const KINDS: [(&str, &str); 2] = [("png", "image/png"), ("jpg", "image/jpeg")];

/// What the bytes actually are, or `None` if they are not an image we accept.
///
/// By magic number, not by the header the client claimed. A `Content-Type` is
/// an assertion by the caller; these bytes get written to disk and served back
/// from the user's own origin, so the assertion is worth nothing.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else {
        None
    }
}

async fn store(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
    Query(params): Query<Params>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let key = key(&k)?;
    if body.len() > MAX_BYTES {
        return Err(Error::invalid("thumbnail is too large").into());
    }
    let ext = sniff(&body).ok_or_else(|| Error::invalid("not a PNG or JPEG"))?;

    // The item has to exist. Without this the cache is a place to park bytes
    // under any name at all, and it outlives every library it belongs to.
    app.store().items.get(lib, &key).await?;

    let path = locate(&cache_dir(&app), &key, &params, ext)?;
    let dir = path.parent().expect("a filename has a parent");
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| Error::internal(format!("could not make the thumbnail cache: {e}")))?;

    // Written beside and renamed: two tabs rendering the same page at once is
    // ordinary, and a half-written PNG served to one of them is not.
    let temp = path.with_extension(format!("{ext}.part"));
    tokio::fs::write(&temp, &body)
        .await
        .map_err(|e| Error::internal(format!("could not write the thumbnail: {e}")))?;
    tokio::fs::rename(&temp, &path)
        .await
        .map_err(|e| Error::internal(format!("could not store the thumbnail: {e}")))?;

    Ok((StatusCode::CREATED, Json(json!({ "stored": body.len(), "kind": ext }))))
}

/// Drop cached pages for items whose bytes are going away.
///
/// Called from the same place attachments are forgotten. A thumbnail that
/// outlives its PDF is a picture of something the library no longer has.
pub async fn forget(app: &App, keys: &[Key]) {
    let dir = cache_dir(app);
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else { return };
    // One pass over the directory rather than a stat per key per width per
    // page: the number of cached pages is unrelated to the number of keys
    // being deleted, and the product of the three is unbounded.
    let doomed: std::collections::HashSet<&str> = keys.iter().map(|k| k.as_str()).collect();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some((owner, _)) = name.split_once("-p") {
            if doomed.contains(owner) {
                tokio::fs::remove_file(entry.path()).await.ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(page: u32, width: u32) -> Params {
        Params { page, width }
    }

    #[test]
    fn a_name_identifies_exactly_one_image() {
        let key: Key = "ABCD1234".parse().unwrap();
        assert_eq!(cache_name(&key, 1, 240, "png"), "ABCD1234-p1-w240.png");
        // Page, width and kind all appear, so nothing that differs can collide
        // — which is what lets the response be marked immutable.
        assert_ne!(cache_name(&key, 1, 240, "png"), cache_name(&key, 2, 240, "png"));
        assert_ne!(cache_name(&key, 1, 240, "png"), cache_name(&key, 1, 480, "png"));
        assert_ne!(cache_name(&key, 1, 240, "png"), cache_name(&key, 1, 240, "jpg"));
    }

    #[test]
    fn the_owner_is_recoverable_from_the_name() {
        // `forget` reads the key back out of the filename, so this is the
        // property that makes one directory pass enough.
        let key: Key = "ABCD1234".parse().unwrap();
        let name = cache_name(&key, 7, 96, "jpg");
        assert_eq!(name.split_once("-p").map(|(k, _)| k), Some("ABCD1234"));
    }

    #[test]
    fn png_and_jpeg_are_recognised_by_their_bytes() {
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0]), Some("png"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
    }

    #[test]
    fn anything_else_is_refused_whatever_it_claims_to_be() {
        // These bytes get served back from the user's own origin, so a
        // Content-Type from the caller is worth nothing.
        assert_eq!(sniff(b"<svg onload=alert(1)>"), None);
        assert_eq!(sniff(b"<!DOCTYPE html>"), None);
        assert_eq!(sniff(b"%PDF-1.7"), None);
        assert_eq!(sniff(b""), None);
        // A PNG signature that is only nearly right.
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0b]), None);
    }

    #[test]
    fn the_client_asks_for_widths_the_server_accepts() {
        // The two lists are in different languages and neither compiler can
        // see the other. If they drift, the client asks for a width the server
        // refuses and covers simply stop appearing — with a 422 in a network
        // panel nobody has open.
        let source = include_str!("../../../../web/src/lib/thumbnails.ts");
        let line = source
            .lines()
            .find(|l| l.contains("THUMB_WIDTHS"))
            .expect("the client still declares its widths");
        for width in WIDTHS {
            assert!(line.contains(&width.to_string()), "the client does not ask for {width}: {line}");
        }
        let client_count = line.matches(char::is_numeric).count();
        assert!(client_count > 0, "no widths found in: {line}");
    }

    #[test]
    fn a_width_nobody_draws_is_refused() {
        let cache = FsPath::new("/tmp/cache");
        let key: Key = "ABCD1234".parse().unwrap();
        // Every distinct width is a file on disk. A range would let one caller
        // fill the cache by counting upwards.
        assert!(locate(cache, &key, &params(1, 241), "png").is_err());
        assert!(locate(cache, &key, &params(1, 240), "png").is_ok());
    }

    #[test]
    fn page_zero_does_not_exist() {
        let cache = FsPath::new("/tmp/cache");
        let key: Key = "ABCD1234".parse().unwrap();
        assert!(locate(cache, &key, &params(0, 240), "png").is_err());
        assert!(locate(cache, &key, &params(10_001, 240), "png").is_err());
        assert!(locate(cache, &key, &params(1, 240), "png").is_ok());
    }
}
