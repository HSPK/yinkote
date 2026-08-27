//! Word, WPS and LibreOffice talk to the library through here.
//!
//! One protocol for every word processor, because the hard part is the same in
//! all of them: the add-in reads the document's citation fields, sends them,
//! and writes back what the server says they should say. See
//! [`document::plan`] for why the whole document travels rather than the one
//! citation being inserted.
//!
//! A session is a working cache, not a record. The document itself is the
//! source of truth — the add-in keeps its fields and its style in a
//! `CustomXmlPart` inside the file — so a session that has been forgotten
//! costs a round trip to open a new one and nothing else. That is what makes
//! it safe to hold them in memory and drop them on restart: the alternative,
//! persisting a table of documents the server has no way to check up on, would
//! rot the first time somebody renamed a file.

pub mod document;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use yk_cite::Format;
use yk_core::model::Item;
use yk_core::{Error, Key};

use crate::error::ApiResult;
use crate::state::App;
use document::{Field, Plan};

/// What a document wants its citations to look like.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    /// A style id from `/citation-styles`.
    #[serde(rename = "styleId")]
    pub style_id: String,
    /// `text` or `html`. A word processor wants HTML so italics survive.
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "html".to_string()
}

impl Default for Prefs {
    fn default() -> Self {
        Self { style_id: "apa".to_string(), format: default_format() }
    }
}

impl Prefs {
    fn format(&self) -> Format {
        if self.format == "text" {
            Format::Text
        } else {
            Format::Html
        }
    }
}

/// The sessions currently open, by id.
///
/// A `Mutex` rather than anything cleverer: the whole critical section is a
/// hash lookup and a clone of two short strings, and there are as many sessions
/// as the author has documents open.
#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

#[derive(Clone, Debug)]
struct Session {
    library: i64,
    prefs: Prefs,
}

impl Sessions {
    /// Open a session for a document, or hand back the one it already has.
    ///
    /// Keyed by the document's own id so that reopening a file — or the add-in
    /// reloading after the server restarted — lands on the same session
    /// instead of leaking a new one per keystroke of bad luck.
    fn open(&self, doc_id: &str, library: i64, prefs: Option<Prefs>) -> (String, Prefs) {
        let id = session_id(doc_id, library);
        let mut sessions = self.0.lock().expect("sessions mutex");
        let session = sessions
            .entry(id.clone())
            .or_insert_with(|| Session { library, prefs: prefs.clone().unwrap_or_default() });
        // A document that arrives carrying preferences is telling the truth
        // about itself: it stored them, and it may have just been opened
        // somewhere this server has never seen.
        //
        // A document that arrives *without* them is saying nothing, which is
        // not the same as asking for the defaults. Treating the two alike
        // silently reset an IEEE thesis to APA the moment the add-in
        // reconnected — every citation in it would have been rewritten.
        if let Some(prefs) = prefs {
            session.prefs = prefs;
        }
        session.library = library;
        (id, session.prefs.clone())
    }

    fn get(&self, id: &str) -> Option<Session> {
        self.0.lock().expect("sessions mutex").get(id).cloned()
    }

    fn set_prefs(&self, id: &str, prefs: Prefs) -> Option<Session> {
        let mut sessions = self.0.lock().expect("sessions mutex");
        let session = sessions.get_mut(id)?;
        session.prefs = prefs;
        Some(session.clone())
    }

    fn close(&self, id: &str) -> bool {
        self.0.lock().expect("sessions mutex").remove(id).is_some()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().expect("sessions mutex").len()
    }
}

/// Stable for a given document, so that reconnecting is free.
fn session_id(doc_id: &str, library: i64) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in doc_id.as_bytes().iter().chain(b"@".iter()).chain(library.to_string().as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn router() -> Router<App> {
    Router::new()
        .route("/integration/session", post(open))
        .route("/integration/session/:sid/cite", post(cite))
        .route("/integration/session/:sid/refresh", post(refresh))
        .route("/integration/session/:sid/bibliography", post(bibliography))
        .route("/integration/session/:sid/prefs", axum::routing::put(set_prefs))
        .route("/integration/session/:sid/close", post(close))
}

#[derive(Deserialize)]
struct OpenBody {
    #[serde(rename = "docId")]
    doc_id: String,
    /// Which library the document cites from. Defaults to the only one most
    /// people have.
    #[serde(default)]
    library: Option<i64>,
    #[serde(rename = "docPrefs", default)]
    prefs: Option<Prefs>,
}

async fn open(State(app): State<App>, Json(body): Json<OpenBody>) -> ApiResult<Json<serde_json::Value>> {
    if body.doc_id.trim().is_empty() {
        return Err(Error::invalid("docId is required").into());
    }
    if let Some(prefs) = &body.prefs {
        check_style(prefs)?;
    }
    let library = body.library.unwrap_or(app.store().default_library);
    let (id, prefs) = app.sessions().open(&body.doc_id, library, body.prefs);
    Ok(Json(json!({ "sessionId": id, "prefs": prefs })))
}

#[derive(Deserialize)]
struct Snapshot {
    /// Every citation field in the document, in document order.
    #[serde(rename = "fieldsSnapshot", default)]
    fields: Vec<Field>,
}

async fn cite(
    State(app): State<App>,
    Path(sid): Path<String>,
    Json(body): Json<Snapshot>,
) -> ApiResult<Json<Plan>> {
    // Inserting and refreshing are the same computation. The add-in has
    // already put the new field into the snapshot it sends, because only the
    // add-in knows where the cursor was.
    render(&app, &sid, body.fields).await
}

async fn refresh(
    State(app): State<App>,
    Path(sid): Path<String>,
    Json(body): Json<Snapshot>,
) -> ApiResult<Json<Plan>> {
    render(&app, &sid, body.fields).await
}

async fn bibliography(
    State(app): State<App>,
    Path(sid): Path<String>,
    Json(body): Json<Snapshot>,
) -> ApiResult<Json<serde_json::Value>> {
    let plan = render(&app, &sid, body.fields).await?;
    let html = plan.0.bibliography.iter().map(|e| e.text.as_str()).collect::<Vec<_>>().join("\n");
    Ok(Json(json!({ "html": html, "entries": plan.0.bibliography })))
}

async fn set_prefs(
    State(app): State<App>,
    Path(sid): Path<String>,
    Json(prefs): Json<Prefs>,
) -> ApiResult<Json<serde_json::Value>> {
    check_style(&prefs)?;
    app.sessions()
        .set_prefs(&sid, prefs.clone())
        .ok_or_else(|| Error::not_found("no such integration session"))?;
    // The style changed, so every citation in the document is now wrong. The
    // add-in follows this with a refresh; saying so here is what tells it to.
    Ok(Json(json!({ "prefs": prefs, "refreshRequired": true })))
}

async fn close(State(app): State<App>, Path(sid): Path<String>) -> Json<serde_json::Value> {
    Json(json!({ "closed": app.sessions().close(&sid) }))
}

/// The shared body of every endpoint that renders: look up the session, fetch
/// the items its fields mention, and plan the document.
async fn render(app: &App, sid: &str, fields: Vec<Field>) -> ApiResult<Json<Plan>> {
    let session =
        app.sessions().get(sid).ok_or_else(|| Error::not_found("no such integration session"))?;
    let style = yk_cite::find(&session.prefs.style_id)
        .ok_or_else(|| Error::invalid(format!("no such citation style: {}", session.prefs.style_id)))?;

    // One request for the whole document, not one per field: a hundred-citation
    // paper would otherwise be a hundred round trips to the database.
    let keys: Vec<Key> = {
        let mut seen: Vec<String> = Vec::new();
        for field in &fields {
            for key in &field.citation.keys {
                if !seen.contains(key) {
                    seen.push(key.clone());
                }
            }
        }
        seen.iter().filter_map(|k| Key::parse(k).ok()).collect()
    };
    let items: HashMap<String, Item> = app
        .store()
        .items
        .get_many(session.library, &keys)
        .await?
        .into_iter()
        .map(|item| (item.key.to_string(), item))
        .collect();

    Ok(Json(document::plan(&fields, &items, style, session.prefs.format())))
}

fn check_style(prefs: &Prefs) -> Result<(), Error> {
    yk_cite::find(&prefs.style_id)
        .map(|_| ())
        .ok_or_else(|| Error::invalid(format!("no such citation style: {}", prefs.style_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_gets_the_same_session_twice() {
        // The add-in reconnects after a restart, or the author closes and
        // reopens the file. Handing out a new session each time would leak one
        // per open and lose the style the document is set to.
        let sessions = Sessions::default();
        let (first, _) = sessions.open("doc-1", 1, Some(Prefs::default()));
        let (again, _) = sessions.open("doc-1", 1, None);
        assert_eq!(first, again);
        assert_eq!(sessions.len(), 1);

        let (other, _) = sessions.open("doc-2", 1, None);
        assert_ne!(first, other, "two documents are two sessions");
    }

    #[test]
    fn the_document_wins_on_reopening() {
        // The file carries its own style. If it says IEEE, it is IEEE, whatever
        // the server happened to remember.
        let sessions = Sessions::default();
        sessions.open("doc-1", 1, Some(Prefs { style_id: "apa".into(), format: "html".into() }));
        let (_, prefs) =
            sessions.open("doc-1", 1, Some(Prefs { style_id: "ieee".into(), format: "html".into() }));
        assert_eq!(prefs.style_id, "ieee");
    }

    #[test]
    fn reconnecting_without_preferences_keeps_the_style() {
        // The add-in reconnects and says nothing about style. Filling that
        // silence with the default would rewrite every citation in an IEEE
        // thesis as APA — which is what it did until a smoke check caught it.
        let sessions = Sessions::default();
        sessions.open("doc-1", 1, Some(Prefs { style_id: "ieee".into(), format: "text".into() }));
        let (_, prefs) = sessions.open("doc-1", 1, None);
        assert_eq!(prefs.style_id, "ieee");
        assert_eq!(prefs.format, "text");
    }

    #[test]
    fn closing_forgets_it() {
        let sessions = Sessions::default();
        let (id, _) = sessions.open("doc-1", 1, Some(Prefs::default()));
        assert!(sessions.close(&id));
        assert!(!sessions.close(&id), "closing twice is not an error, but it is not a session");
        assert!(sessions.get(&id).is_none());
    }
}
