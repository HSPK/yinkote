//! Where somebody left off in a document.
//!
//! A forty-page paper reopened at page one is a small thing that happens every
//! day, and it is the difference between a reader and a PDF viewer.
//!
//! Kept in the settings table under `reader.<key>` rather than on the
//! attachment itself. Writing it to the item would bump the library version on
//! every scroll, and the version is what drives sync and cache invalidation —
//! reading a paper would look like editing the library.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::key;
use crate::error::ApiResult;
use crate::state::App;

/// How a document was left.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderState {
    /// 1-based, as printed on the page and as the reader counts.
    #[serde(rename = "lastPage", default = "first_page")]
    pub last_page: u32,
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    /// `paged` or `continuous`.
    #[serde(rename = "scrollMode", default = "default_scroll")]
    pub scroll_mode: String,
    /// Whether the annotations pane was open.
    #[serde(default = "yes")]
    pub sidebar: bool,
}

fn first_page() -> u32 {
    1
}
fn default_zoom() -> f32 {
    1.2
}
fn default_scroll() -> String {
    "continuous".into()
}
fn yes() -> bool {
    true
}

impl Default for ReaderState {
    fn default() -> Self {
        Self {
            last_page: first_page(),
            zoom: default_zoom(),
            scroll_mode: default_scroll(),
            sidebar: yes(),
        }
    }
}

impl ReaderState {
    /// Bring a stored or submitted state into range.
    ///
    /// A zoom of zero renders nothing and a page of zero does not exist; both
    /// are what arrives when a client sends a field it has not filled in yet,
    /// and neither should be stored for the next reader to trip over.
    pub fn sane(mut self) -> Self {
        self.last_page = self.last_page.max(1);
        self.zoom = self.zoom.clamp(0.25, 8.0);
        if self.scroll_mode != "paged" {
            self.scroll_mode = "continuous".into();
        }
        self
    }
}

fn setting_key(attachment: &str) -> String {
    format!("reader.{attachment}")
}

pub fn router() -> Router<App> {
    Router::new().route(
        "/libraries/:lib/items/:key/reader-state",
        get(read).put(write),
    )
}

async fn read(
    State(app): State<App>,
    Path((_lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<ReaderState>> {
    // The key is parsed even though it is only used as a string, so a
    // malformed one is refused rather than becoming a settings row nothing
    // will ever read again.
    let attachment = key(&k)?;
    let stored = app.store().settings.get(&setting_key(attachment.as_str())).await?;
    Ok(Json(
        stored
            .and_then(|v| serde_json::from_value::<ReaderState>(v).ok())
            .unwrap_or_default()
            .sane(),
    ))
}

async fn write(
    State(app): State<App>,
    Path((_lib, k)): Path<(i64, String)>,
    Json(state): Json<ReaderState>,
) -> ApiResult<Json<serde_json::Value>> {
    let attachment = key(&k)?;
    let state = state.sane();
    // `to_value` on a struct of numbers and strings cannot fail; `json!` says
    // so without asking every caller to handle an error that cannot happen.
    let stored = json!({
        "lastPage": state.last_page,
        "zoom": state.zoom,
        "scrollMode": state.scroll_mode,
        "sidebar": state.sidebar,
    });
    app.store().settings.set(&setting_key(attachment.as_str()), &stored).await?;
    // No `announce`: this is not a change to the library. Broadcasting it would
    // make every other tab reload its list because somebody scrolled.
    Ok(Json(json!({ "saved": true, "state": state })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_state_reads_as_the_top_of_the_document() {
        let fresh = ReaderState::default().sane();
        assert_eq!(fresh.last_page, 1);
        assert_eq!(fresh.scroll_mode, "continuous");
    }

    #[test]
    fn nonsense_is_brought_into_range_rather_than_stored() {
        // What arrives from a client that has not finished loading: a zoom of
        // zero renders nothing, and page zero does not exist.
        let broken = ReaderState { last_page: 0, zoom: 0.0, scroll_mode: "sideways".into(), sidebar: false };
        let fixed = broken.sane();
        assert_eq!(fixed.last_page, 1);
        assert_eq!(fixed.zoom, 0.25);
        assert_eq!(fixed.scroll_mode, "continuous", "an unknown mode is the ordinary one");
        assert!(!fixed.sidebar, "but a real choice is left alone");
    }

    #[test]
    fn an_absurd_zoom_is_clamped_at_both_ends() {
        assert_eq!(ReaderState { zoom: 500.0, ..Default::default() }.sane().zoom, 8.0);
        assert_eq!(ReaderState { zoom: -3.0, ..Default::default() }.sane().zoom, 0.25);
    }

    #[test]
    fn a_partial_body_keeps_the_defaults_for_what_it_omits() {
        // The client sends the page it is on and nothing else, which is the
        // common case while scrolling.
        let state: ReaderState = serde_json::from_value(json!({ "lastPage": 12 })).unwrap();
        assert_eq!(state.last_page, 12);
        assert_eq!(state.zoom, 1.2);
        assert!(state.sidebar);
    }

    #[test]
    fn state_is_filed_under_the_attachment_it_belongs_to() {
        assert_eq!(setting_key("ABCD1234"), "reader.ABCD1234");
        assert_ne!(setting_key("ABCD1234"), setting_key("EFGH5678"));
    }
}
