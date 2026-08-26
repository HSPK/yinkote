//! HTTP-level tests.
//!
//! These drive the real router with a real (in-memory) store, so they cover the
//! wiring that unit tests cannot: extractors, status-code mapping, header
//! handling and the JSON shapes the frontend actually consumes.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;
use yk_server::config::Config;
use yk_server::state::App;
use yk_store::Store;

struct Client {
    router: axum::Router,
    dir: std::path::PathBuf,
}

impl Drop for Client {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

impl Client {
    async fn new() -> (Self, App) {
        let dir = std::env::temp_dir().join(format!(
            "yk-api-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config { data_dir: Some(dir.clone()), ..Default::default() };
        let app = yk_server::build_with_store(config, Store::in_memory().unwrap())
            .await
            .expect("build app");
        (Self { router: yk_server::router(app.clone()), dir }, app)
    }

    fn request(&self, method: &str, path: &str, body: Option<Value>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1{path}"))
            .header("host", "127.0.0.1:23130");
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        builder.body(body.map_or(Body::empty(), |b| Body::from(b.to_string()))).unwrap()
    }

    async fn send(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let response = self.router.clone().oneshot(self.request(method, path, body)).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn get(&self, path: &str) -> Value {
        let (status, body) = self.send("GET", path, None).await;
        assert!(status.is_success(), "GET {path} -> {status}: {body}");
        body
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        let (status, body) = self.send("POST", path, Some(body)).await;
        assert!(status.is_success(), "POST {path} -> {status}: {body}");
        body
    }
}

fn article(title: &str) -> Value {
    json!({
        "itemType": "journalArticle",
        "title": title,
        "abstractNote": format!("An abstract mentioning {title} and transformers."),
        "date": "2021-03-04",
        "creators": [{ "creatorType": "author", "firstName": "Ada", "lastName": "Lovelace" }],
        "tags": [{ "tag": "demo" }]
    })
}

#[tokio::test]
async fn ping_reports_service_metadata() {
    let (c, _) = Client::new().await;
    let body = c.get("/ping").await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["service"], "yinkote");
    assert!(body["defaultLibrary"].as_i64().unwrap() >= 1);
}

#[tokio::test]
async fn schema_is_served_for_the_frontend() {
    let (c, _) = Client::new().await;
    let body = c.get("/schema").await;
    assert!(body["itemTypes"].as_array().unwrap().len() > 10);
    assert!(body["fields"]["title"]["label"].is_string());
}

#[tokio::test]
async fn create_read_update_round_trip() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let created = c.post(&format!("/libraries/{lib}/items"), json!([article("Alpha")])).await;
    assert_eq!(created["created"].as_array().unwrap().len(), 1);
    let key = created["created"][0]["key"].as_str().unwrap().to_string();
    let version = created["created"][0]["version"].as_i64().unwrap();

    let fetched = c.get(&format!("/libraries/{lib}/items/{key}")).await;
    assert_eq!(fetched["title"], "Alpha");
    assert_eq!(fetched["creators"][0]["lastName"], "Lovelace");

    let (status, updated) = c
        .send(
            "PATCH",
            &format!("/libraries/{lib}/items/{key}"),
            Some(json!({ "fields": { "volume": "42" } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["volume"], "42");
    assert!(updated["version"].as_i64().unwrap() > version);
}

#[tokio::test]
async fn batch_create_isolates_a_bad_row() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let body = c
        .post(
            &format!("/libraries/{lib}/items"),
            json!([article("Good"), { "itemType": "nonsense", "title": "Bad" }]),
        )
        .await;
    assert_eq!(body["created"].as_array().unwrap().len(), 1);
    assert_eq!(body["failed"][0]["index"], 1);
    assert_eq!(body["failed"][0]["code"], "invalid_input");
}

#[tokio::test]
async fn stale_write_is_rejected_with_412() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let created = c.post(&format!("/libraries/{lib}/items"), json!([article("Alpha")])).await;
    let key = created["created"][0]["key"].as_str().unwrap().to_string();
    let stale = created["created"][0]["version"].as_i64().unwrap();

    let precondition = |value: &str| {
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/libraries/{lib}/items/{key}"))
            .header("host", "127.0.0.1:23130")
            .header("content-type", "application/json")
            .header("If-Unmodified-Since-Version", stale.to_string())
            .body(Body::from(json!({ "fields": { "volume": value } }).to_string()))
            .unwrap()
    };

    // The first write matches the precondition and moves the version on.
    let ok = c.router.clone().oneshot(precondition("1")).await.unwrap();
    assert!(ok.status().is_success());

    // Replaying the same precondition must now fail rather than clobber.
    let stale_response = c.router.clone().oneshot(precondition("2")).await.unwrap();
    assert_eq!(stale_response.status(), StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn errors_map_to_meaningful_status_codes() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let (status, body) = c.send("GET", &format!("/libraries/{lib}/items/ZZZZZZZZ"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");

    let (status, _) = c.send("GET", &format!("/libraries/{lib}/items/not%20a%20key"), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = c.send("GET", "/plugins/does-not-exist", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dns_rebinding_is_rejected() {
    let (c, _) = Client::new().await;
    let request = Request::builder()
        .uri("/api/v1/ping")
        .header("host", "evil.example.com")
        .body(Body::empty())
        .unwrap();
    let response = c.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn collections_scope_the_item_list() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let coll = c.post(&format!("/libraries/{lib}/collections"), json!({ "name": "Reading" })).await;
    let ckey = coll["key"].as_str().unwrap().to_string();

    let mut inside = article("Inside");
    inside["collections"] = json!([ckey]);
    c.post(&format!("/libraries/{lib}/items"), json!([inside, article("Outside")])).await;

    assert_eq!(c.get(&format!("/libraries/{lib}/items")).await["total"], 2);

    let scoped = c.get(&format!("/libraries/{lib}/items?collection={ckey}")).await;
    assert_eq!(scoped["total"], 1);
    assert_eq!(scoped["items"][0]["title"], "Inside");

    let listed = c.get(&format!("/libraries/{lib}/collections")).await;
    assert_eq!(listed[0]["itemCount"], 1);
}

#[tokio::test]
async fn search_returns_hits_with_provenance() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("Attention")])).await;

    let body = c.get(&format!("/libraries/{lib}/search?q=attention&mode=keyword")).await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 1);
    assert_eq!(body["hits"][0]["sources"][0], "keyword");
    assert!(body["tookMs"].is_number());

    // The same query through the item list hydrates full records.
    let hydrated = c.get(&format!("/libraries/{lib}/items?q=attention")).await;
    assert_eq!(hydrated["items"][0]["title"], "Attention");
    assert!(hydrated["items"][0]["match"]["snippet"].as_str().unwrap().contains("<mark>"));
}

#[tokio::test]
async fn search_query_operators_filter_results() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let mut tagged = article("Tagged");
    tagged["tags"] = json!([{ "tag": "survey" }]);
    c.post(&format!("/libraries/{lib}/items"), json!([tagged, article("Untagged")])).await;

    let hits = c.get(&format!("/libraries/{lib}/search?q=tag:survey")).await;
    assert_eq!(hits["hits"].as_array().unwrap().len(), 1);

    let excluded = c.get(&format!("/libraries/{lib}/search?q=tag:survey%20-tag:survey")).await;
    assert!(excluded["hits"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn trash_hides_then_restores_items() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let created = c.post(&format!("/libraries/{lib}/items"), json!([article("Doomed")])).await;
    let key = created["created"][0]["key"].as_str().unwrap().to_string();

    let (status, body) =
        c.send("DELETE", &format!("/libraries/{lib}/items"), Some(json!({ "keys": [key] }))).await;
    assert!(status.is_success(), "{body}");
    assert_eq!(body["trashed"], 1);

    assert_eq!(c.get(&format!("/libraries/{lib}/items")).await["total"], 0);
    assert_eq!(c.get(&format!("/libraries/{lib}/items?trash=only")).await["total"], 1);
    assert!(
        c.get(&format!("/libraries/{lib}/search?q=doomed")).await["hits"]
            .as_array()
            .unwrap()
            .is_empty(),
        "trashed items must leave the search index"
    );

    c.post(&format!("/libraries/{lib}/items/restore"), json!({ "keys": [key] })).await;
    assert_eq!(c.get(&format!("/libraries/{lib}/items")).await["total"], 1);
}

#[tokio::test]
async fn tags_and_facets_reflect_the_library() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("One"), article("Two")])).await;

    let tags = c.get(&format!("/libraries/{lib}/tags")).await;
    assert_eq!(tags[0]["name"], "demo");
    assert_eq!(tags[0]["count"], 2);

    let facets = c.get(&format!("/libraries/{lib}/facets")).await;
    assert_eq!(facets[0]["count"], 2);
}

#[tokio::test]
async fn listing_advertises_the_library_version() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("One")])).await;

    let response = c.router.clone().oneshot(c.request("GET", &format!("/libraries/{lib}/items"), None)).await.unwrap();
    let version = response
        .headers()
        .get("Last-Modified-Version")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .expect("Last-Modified-Version header");
    assert!(version >= 1);
}

#[tokio::test]
async fn since_returns_only_what_changed() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("Old")])).await;
    let watermark = c.get("/stats").await["version"].as_i64().unwrap();
    c.post(&format!("/libraries/{lib}/items"), json!([article("New")])).await;

    let delta = c.get(&format!("/libraries/{lib}/items?since={watermark}")).await;
    assert_eq!(delta["total"], 1);
    assert_eq!(delta["items"][0]["title"], "New");
}

#[tokio::test]
async fn plugin_endpoints_answer_without_plugins_installed() {
    let (c, _) = Client::new().await;
    assert!(c.get("/plugins").await.as_array().unwrap().is_empty());
    assert!(c.get("/plugins/contributions").await["metadataSources"].as_array().unwrap().is_empty());

    let (status, _) =
        c.send("POST", "/plugins/dispatch", Some(json!({ "name": "not.a.hook" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown hooks are rejected");
}

#[tokio::test]
async fn settings_round_trip_and_stay_namespaced() {
    let (c, _) = Client::new().await;
    c.send("PUT", "/settings", Some(json!({ "theme": "dark", "ui.density": "compact" }))).await;
    let body = c.get("/settings").await;
    assert_eq!(body["ui.theme"], "dark", "bare keys are namespaced under ui.");
    assert_eq!(body["ui.density"], "compact");
}

#[tokio::test]
async fn stats_summarise_the_library() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("One")])).await;

    let stats = c.get("/stats").await;
    assert_eq!(stats["items"], 1);
    assert_eq!(stats["trashed"], 0);
    assert_eq!(stats["search"]["provider"], "local-hash");
}

#[tokio::test]
async fn reindex_rebuilds_search_from_the_items_table() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("Rebuildable")])).await;

    let body = c.post(&format!("/maintenance/reindex/{lib}"), json!({})).await;
    assert_eq!(body["reindexed"], 1);
    assert_eq!(
        c.get(&format!("/libraries/{lib}/search?q=rebuildable")).await["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn conversations_keep_their_transcript_and_recency_order() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let base = format!("/libraries/{lib}/conversations");

    let first = c.post(&base, json!({ "title": "Retrieval" })).await;
    let second = c.post(&base, json!({})).await;
    assert_eq!(second["messageCount"], 0);

    let key = first["key"].as_str().unwrap().to_string();
    c.post(&format!("{base}/{key}/messages"), json!({ "role": "user", "content": "Why RRF?" }))
        .await;

    let messages = c.get(&format!("{base}/{key}/messages")).await;
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["content"], "Why RRF?");

    // Appending is activity, so the thread that was just used sorts first.
    let list = c.get(&base).await;
    assert_eq!(list[0]["key"], key.as_str());
    assert_eq!(list[0]["messageCount"], 1);

    let renamed = c.send("PATCH", &format!("{base}/{key}"), Some(json!({ "title": "Fusion" }))).await;
    assert_eq!(renamed.1["title"], "Fusion");

    let gone = c.send("DELETE", &format!("{base}/{key}"), None).await;
    assert_eq!(gone.1["deleted"], 1);
    assert_eq!(c.get(&base).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn unknown_conversations_are_not_found() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let (status, _) =
        c.send("GET", &format!("/libraries/{lib}/conversations/NOPE1234"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn collections_reparent_and_return_to_the_top_level() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let base = format!("/libraries/{lib}/collections");

    let parent = c.post(&base, json!({ "name": "Parent" })).await;
    let child = c.post(&base, json!({ "name": "Child" })).await;
    let (pk, ck) = (parent["key"].as_str().unwrap(), child["key"].as_str().unwrap());

    let nested = c.send("PATCH", &format!("{base}/{ck}"), Some(json!({ "parentKey": pk }))).await;
    assert_eq!(nested.1["parentKey"], pk, "dropping onto a collection nests it");

    // An explicit null is the "drop onto the library root" gesture.
    let freed = c.send("PATCH", &format!("{base}/{ck}"), Some(json!({ "parentKey": null }))).await;
    assert!(freed.1["parentKey"].is_null(), "null must clear the parent, not be ignored");

    let renamed =
        c.send("PATCH", &format!("{base}/{ck}"), Some(json!({ "name": "Renamed" }))).await;
    assert!(renamed.1["parentKey"].is_null(), "an absent parentKey changes nothing");

    let (status, _) =
        c.send("PATCH", &format!("{base}/{pk}"), Some(json!({ "parentKey": pk }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "a collection cannot contain itself");
}

#[tokio::test]
async fn sorting_by_a_badge_needs_a_plugin_and_degrades_without_one() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("Unranked")])).await;

    // No badge plugin is loaded in tests, so nothing can be ranked. The list
    // must still come back — an unorderable column is not a broken request.
    let (status, body) = c
        .send("GET", &format!("/libraries/{lib}/items?sort=badge:metrics:if"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_malformed_badge_sort_falls_back_to_an_ordinary_one() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    c.post(&format!("/libraries/{lib}/items"), json!([article("Anything")])).await;

    for sort in ["badge:", "badge:metrics", "badge::if"] {
        let (status, body) =
            c.send("GET", &format!("/libraries/{lib}/items?sort={sort}"), None).await;
        assert_eq!(status, StatusCode::OK, "{sort} must not fail the request");
        assert_eq!(body["items"].as_array().unwrap().len(), 1, "{sort}");
    }
}

#[tokio::test]
async fn the_agent_reports_itself_unconfigured_rather_than_pretending() {
    let (c, _) = Client::new().await;
    let body = c.get("/agent").await;
    assert_eq!(body["configured"], false, "no model is configured by default");
    assert!(body["model"].is_null());
}

#[tokio::test]
async fn asking_without_a_model_still_records_the_question() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let base = format!("/libraries/{lib}/conversations");
    let key = c.post(&base, json!({ "title": "Ask" })).await["key"].as_str().unwrap().to_string();

    let (status, _) =
        c.send("POST", &format!("{base}/{key}/ask"), Some(json!({ "content": "why RRF?" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "the caller is told what is missing");

    // What the user typed must survive a failure to answer it.
    let messages = c.get(&format!("{base}/{key}/messages")).await;
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[tokio::test]
async fn an_empty_question_is_rejected_before_anything_is_stored() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let base = format!("/libraries/{lib}/conversations");
    let key = c.post(&base, json!({})).await["key"].as_str().unwrap().to_string();

    let (status, _) =
        c.send("POST", &format!("{base}/{key}/ask"), Some(json!({ "content": "  " }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(c.get(&format!("{base}/{key}/messages")).await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn annotations_are_ordinary_child_items() {
    // The schema drives the item types, so highlighting a PDF needs no new
    // storage, no new endpoint and no migration — only a new type in the JSON.
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let paper = c.post(&format!("/libraries/{lib}/items"), json!([article("Host paper")])).await
        ["created"][0]["key"]
        .as_str()
        .unwrap()
        .to_string();

    let attachment = c
        .post(
            &format!("/libraries/{lib}/items"),
            json!([{ "itemType": "attachment", "parentKey": paper, "filename": "p.pdf" }]),
        )
        .await["created"][0]["key"]
        .as_str()
        .unwrap()
        .to_string();

    let made = c
        .post(
            &format!("/libraries/{lib}/items"),
            json!([{
                "itemType": "annotation",
                "parentKey": attachment,
                "annotationType": "highlight",
                "annotationText": "attention is all you need",
                "annotationColor": "amber",
                "annotationPage": "3",
                "annotationPosition": "{\"rects\":[[10,20,30,40]]}",
            }]),
        )
        .await;
    assert_eq!(made["created"][0]["annotationText"], "attention is all you need");

    let children = c.get(&format!("/libraries/{lib}/items/{attachment}/children")).await;
    assert_eq!(children.as_array().unwrap().len(), 1);

    // Annotations must not clutter the item list; they belong to their file.
    let top = c.get(&format!("/libraries/{lib}/items?topLevel=true")).await;
    assert_eq!(top["total"], 1, "only the paper is top level");

    // …but their text is searchable, which is the point of storing it.
    let hits = c.get(&format!("/libraries/{lib}/search?q=attention")).await;
    assert!(!hits["hits"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn creating_many_items_dispatches_one_hook_for_the_batch() {
    // The regression this guards: the hook fired once per draft, so a 500-item
    // import made 500 sequential round-trips to every subscriber while holding
    // the request — and behind it the write lock — open. At scale that is the
    // difference between an import finishing and the database reporting a
    // busy timeout.
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let batch: Vec<_> = (0..50).map(|i| article(&format!("Batch item {i}"))).collect();
    let body = c.post(&format!("/libraries/{lib}/items"), json!(batch)).await;

    assert_eq!(body["created"].as_array().unwrap().len(), 50);
    assert!(body["failed"].as_array().unwrap().is_empty());
}
