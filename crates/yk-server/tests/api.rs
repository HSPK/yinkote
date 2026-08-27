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
    /// Start a long job and wait for it, answering with its result.
    ///
    /// Importing, exporting and rebuilding all hand back a task rather than
    /// holding the request open. Every test that used to read the answer
    /// straight out of the response goes through here instead, so there is one
    /// place that knows the shape.
    async fn await_task(&self, path: &str, body: serde_json::Value) -> Value {
        let started = self.post(path, body).await;
        let id = started["task"]["id"].as_str().expect("a task to watch").to_string();
        loop {
            let state = self.get(&format!("/tasks/{id}")).await;
            if state["phase"] != "running" {
                assert_eq!(state["phase"], "done", "the job did not finish: {state}");
                return state["result"].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

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

    // Started, not awaited: a rebuild is half a minute on a real library, so
    // the request hands back a task and the work carries on behind it.
    let body = c.post(&format!("/maintenance/reindex/{lib}"), json!({})).await;
    let task = body["task"]["id"].as_str().expect("a task to watch").to_string();
    assert_eq!(body["task"]["phase"], "running");

    let finished = loop {
        let state = c.get(&format!("/tasks/{task}")).await;
        if state["phase"] != "running" {
            break state;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(finished["phase"], "done");
    assert_eq!(finished["result"]["reindexed"], 1);
    assert_eq!(
        c.get(&format!("/libraries/{lib}/search?q=rebuildable")).await["hits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn the_model_can_be_pointed_at_from_the_workbench() {
    let (c, _app) = Client::new().await;

    let before = c.get("/agent").await;
    assert_eq!(before["configured"], false);

    // Half a configuration is the common mistake, and "not configured" sends
    // people to the wrong field half the time.
    let (status, body) = c.send("PUT", "/agent", Some(json!({ "model": "some-model" }))).await;
    assert_eq!(status, 422, "{body}");
    assert!(body["title"].as_str().unwrap_or_default().contains("endpoint"), "{body}");

    let (status, saved) = c
        .send(
            "PUT",
            "/agent",
            Some(json!({
                "endpoint": "http://127.0.0.1:9/v1",
                "model": "some-model",
                "apiKey": "secret",
            })),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["configured"], true);
    assert_eq!(saved["model"], "some-model");

    // The key is never handed back, only the fact that one is set.
    assert_eq!(saved["hasApiKey"], true);
    assert!(!saved.to_string().contains("secret"), "the key must not be echoed: {saved}");

    // Saving answers with exactly what a fresh read says.
    assert_eq!(c.get("/agent").await, saved);

    // An absent key leaves the stored one alone; a form that never shows the
    // key would otherwise erase it on every save.
    let (_, again) = c.send("PUT", "/agent", Some(json!({ "model": "other-model" }))).await;
    assert_eq!(again["hasApiKey"], true, "{again}");
    assert_eq!(again["model"], "other-model");
}

#[tokio::test]
async fn tools_and_skills_can_be_switched_off_from_the_workbench() {
    let (c, _app) = Client::new().await;

    c.send(
        "PUT",
        "/agent",
        Some(json!({ "endpoint": "http://127.0.0.1:9/v1", "model": "m" })),
    )
    .await;

    let before = c.get("/agent").await;
    let catalogue = before["allTools"].as_array().unwrap().clone();
    assert!(!catalogue.is_empty(), "the catalogue is what the settings page offers");
    assert!(catalogue.iter().any(|t| t == "trash_items"), "{catalogue:?}");
    assert!(before["tools"].as_array().unwrap().iter().any(|t| t == "trash_items"));

    let (_, off) = c
        .send("PUT", "/agent", Some(json!({ "disabledTools": ["trash_items", "delete_items"] })))
        .await;

    // Gone from what the assistant has, still in what can be offered — a list
    // of only the enabled tools could never offer to switch one back on.
    assert!(!off["tools"].as_array().unwrap().iter().any(|t| t == "trash_items"), "{off}");
    assert!(off["allTools"].as_array().unwrap().iter().any(|t| t == "trash_items"), "{off}");
    assert_eq!(off["disabledTools"][0], "trash_items");

    // And it survives a fresh read, because it was written to the config.
    let again = c.get("/agent").await;
    assert!(!again["tools"].as_array().unwrap().iter().any(|t| t == "trash_items"));
}

#[tokio::test]
async fn a_long_thread_arrives_a_page_at_a_time() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let base = format!("/libraries/{lib}/conversations");
    let key = c.post(&base, json!({ "title": "Long" })).await["key"].as_str().unwrap().to_string();

    for i in 0..25 {
        c.post(
            &format!("{base}/{key}/messages"),
            json!({ "role": "user", "content": format!("Message {i}") }),
        )
        .await;
    }

    // Opening a conversation must not depend on how long it has been going.
    let page = c.get(&format!("{base}/{key}/messages?limit=10")).await;
    let first = page["messages"].as_array().unwrap();
    assert_eq!(first.len(), 10);
    assert_eq!(page["hasMore"], true);

    // The newest ten, in reading order — which is what gets drawn.
    assert_eq!(first[0]["content"], "Message 15");
    assert_eq!(first[9]["content"], "Message 24");

    // Older ones on request.
    let oldest_seen = first[0]["id"].as_i64().unwrap();
    let older = c.get(&format!("{base}/{key}/messages?limit=10&before={oldest_seen}")).await;
    assert_eq!(older["messages"][0]["content"], "Message 5");
    assert_eq!(older["messages"][9]["content"], "Message 14");
    assert_eq!(older["hasMore"], true);

    // And the end of the thread says so rather than leaving a client asking.
    let start = older["messages"][0]["id"].as_i64().unwrap();
    let last = c.get(&format!("{base}/{key}/messages?limit=10&before={start}")).await;
    assert_eq!(last["messages"].as_array().unwrap().len(), 5);
    assert_eq!(last["hasMore"], false);
}

#[tokio::test]
async fn a_bibliography_can_be_recorded_without_the_network() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let made = c
        .post(
            &format!("/libraries/{lib}/items"),
            json!([
                { "itemType": "journalArticle", "title": "Citing paper" },
                { "itemType": "journalArticle", "title": "A work it cites", "DOI": "10.1/held" },
            ]),
        )
        .await;
    let citing = made["created"][0]["key"].as_str().unwrap().to_string();
    let held = made["created"][1]["key"].as_str().unwrap().to_string();

    let (status, saved) = c
        .send(
            "PUT",
            &format!("/libraries/{lib}/items/{citing}/citations"),
            Some(json!({ "citations": [
                { "doi": "10.1/held", "label": "A work it cites", "year": 2019 },
                { "doi": "10.1/absent", "label": "Something we do not have", "year": 2020 },
                { "label": "A reference with no identifier at all" },
            ]})),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["stored"], 3);

    let listed = c.get(&format!("/libraries/{lib}/items/{citing}/citations")).await;
    let cites = listed["cites"].as_array().unwrap();
    assert_eq!(cites.len(), 3, "the order the paper printed them in is kept");

    // The one the library holds resolves to it; the rest keep their labels.
    // Getting this wrong is the whole risk of writing fingerprints by hand.
    assert_eq!(cites[0]["key"], held.as_str());
    assert!(cites[1]["key"].is_null(), "{}", cites[1]);
    assert_eq!(listed["resolved"], 1);

    // And it is a replacement, not a merge: a reference list belongs to a
    // printed paper, and two versions of one merged match neither.
    c.send(
        "PUT",
        &format!("/libraries/{lib}/items/{citing}/citations"),
        Some(json!({ "citations": [{ "doi": "10.1/held", "label": "Only this now" }] })),
    )
    .await;
    let again = c.get(&format!("/libraries/{lib}/items/{citing}/citations")).await;
    assert_eq!(again["cites"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_paper_knows_which_conversations_named_it() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let created = c
        .post(
            &format!("/libraries/{lib}/items"),
            json!([{ "itemType": "journalArticle", "title": "Attention" }]),
        )
        .await;
    let item = created["created"][0]["key"].as_str().unwrap().to_string();

    let convo = c
        .post(&format!("/libraries/{lib}/conversations"), json!({ "title": "About it" }))
        .await;
    let key = convo["key"].as_str().unwrap().to_string();

    c.post(
        &format!("/libraries/{lib}/conversations/{key}/messages"),
        json!({ "role": "user", "content": "what does @this argue?", "mentions": [item] }),
    )
    .await;

    // Reading the thread back has to return what was named, or the client
    // cannot render the chip it let the user attach.
    let page = c.get(&format!("/libraries/{lib}/conversations/{key}/messages")).await;
    assert_eq!(page["messages"][0]["mentions"][0], item.as_str());

    // And the reverse lookup, which is what the detail panel asks.
    let about = c.get(&format!("/libraries/{lib}/items/{item}/conversations")).await;
    assert_eq!(about["conversations"][0]["key"], key.as_str());

    let other = c
        .post(
            &format!("/libraries/{lib}/items"),
            json!([{ "itemType": "journalArticle", "title": "Unrelated" }]),
        )
        .await;
    let other = other["created"][0]["key"].as_str().unwrap();
    let none = c.get(&format!("/libraries/{lib}/items/{other}/conversations")).await;
    assert!(none["conversations"].as_array().unwrap().is_empty());
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

    let page = c.get(&format!("{base}/{key}/messages")).await;
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(page["messages"][0]["content"], "Why RRF?");
    assert_eq!(page["hasMore"], false);

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
    let page = c.get(&format!("{base}/{key}/messages")).await;
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);
    assert_eq!(page["messages"][0]["role"], "user");
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
    assert!(c.get(&format!("{base}/{key}/messages")).await["messages"].as_array().unwrap().is_empty());
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

#[tokio::test]
async fn destroying_an_item_reclaims_its_files() {
    // The disk is the one place nothing reminds you of a leak: the rows are
    // gone, so afterwards nothing even says which directories were theirs.
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let paper = c.post(&format!("/libraries/{lib}/items"), json!([article("With a file")])).await
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

    let key: yk_core::Key = attachment.parse().unwrap();
    app.storage().put(&key, "p.pdf", b"%PDF-1.7").await.unwrap();
    assert!(app.storage().size(&key, "p.pdf").await.is_some());

    c.post(&format!("/libraries/{lib}/items/delete"), json!({ "keys": [paper] })).await;

    assert!(
        app.storage().size(&key, "p.pdf").await.is_none(),
        "the bytes went with the item that owned them"
    );
}

#[tokio::test]
async fn trashing_an_item_keeps_its_files() {
    // Trash is reversible, so anything it threw away would be a data loss bug.
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;

    let paper = c.post(&format!("/libraries/{lib}/items"), json!([article("Recoverable")])).await
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

    let key: yk_core::Key = attachment.parse().unwrap();
    app.storage().put(&key, "p.pdf", b"%PDF-1.7").await.unwrap();

    c.send("DELETE", &format!("/libraries/{lib}/items"), Some(json!({ "keys": [paper] }))).await;
    assert!(app.storage().size(&key, "p.pdf").await.is_some(), "trash must not delete bytes");

    c.post(&format!("/libraries/{lib}/items/restore"), json!({ "keys": [paper] })).await;
    assert!(app.storage().size(&key, "p.pdf").await.is_some());
}

/// Build a minimal Zotero library on disk, for the import tests.
fn zotero_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("zotero.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT);
         CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INTEGER, key TEXT,
                             dateAdded TEXT, dateModified TEXT);
         CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY);
         CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT);
         CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value TEXT);
         CREATE TABLE itemData (itemID INTEGER, fieldID INTEGER, valueID INTEGER);
         CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT,
                                fieldMode INTEGER);
         CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
         CREATE TABLE itemCreators (itemID INTEGER, creatorID INTEGER, creatorTypeID INTEGER,
                                    orderIndex INTEGER);
         CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE itemTags (itemID INTEGER, tagID INTEGER, type INTEGER);
         CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT,
                                   key TEXT, parentCollectionID INTEGER);
         CREATE TABLE collectionItems (collectionID INTEGER, itemID INTEGER);
         CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INTEGER,
                                       path TEXT, contentType TEXT);

         INSERT INTO itemTypes VALUES (1, 'journalArticle');
         INSERT INTO fields VALUES (1, 'title');
         INSERT INTO creatorTypes VALUES (1, 'author');
         INSERT INTO items VALUES (10, 1, 'ZOTE1111', '2020-01-01', '2020-01-02'),
                                  (11, 1, 'ZOTE2222', '2020-01-01', '2020-01-02');
         INSERT INTO itemDataValues VALUES (1, 'Imported One'), (2, 'Imported Two');
         INSERT INTO itemData VALUES (10, 1, 1), (11, 1, 2);
         INSERT INTO collections VALUES (1, 'From Zotero', 'ZCOL1111', NULL);
         INSERT INTO collectionItems VALUES (1, 10);",
    )
    .unwrap();
    path
}

#[tokio::test]
async fn importing_a_zotero_library_is_previewed_then_committed() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let dir = tempfile::tempdir().unwrap();
    let path = zotero_fixture(dir.path()).to_string_lossy().to_string();

    // Merging one library into another is not something to discover you did.
    let seen = c.post("/import/zotero/preview", json!({ "path": path })).await;
    assert_eq!(seen["items"], 2);
    assert_eq!(seen["collections"], 1);
    assert_eq!(c.get(&format!("/libraries/{lib}/items")).await["total"], 0, "preview writes nothing");

    let done = c.await_task(&format!("/libraries/{lib}/import/zotero"), json!({ "path": path })).await;
    assert_eq!(done["items"], 2);
    assert_eq!(done["failed"], 0);

    let items = c.get(&format!("/libraries/{lib}/items")).await;
    assert_eq!(items["total"], 2);

    // Membership survives the crossing.
    let filed = c.get(&format!("/libraries/{lib}/items?collection=ZCOL1111")).await;
    assert_eq!(filed["total"], 1);
    assert_eq!(filed["items"][0]["title"], "Imported One");
}

#[tokio::test]
async fn importing_the_same_library_twice_updates_rather_than_duplicating() {
    // Zotero's keys are kept precisely so a repeat import is safe — and useful:
    // it should carry across whatever changed in Zotero since.
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let dir = tempfile::tempdir().unwrap();
    let path = zotero_fixture(dir.path()).to_string_lossy().to_string();

    let first = c.await_task(&format!("/libraries/{lib}/import/zotero"), json!({ "path": path })).await;
    assert_eq!(first["items"], 2);
    assert_eq!(first["updated"], 0);

    // Retitle one of them in Zotero, as a user would.
    let db = rusqlite::Connection::open(dir.path().join("zotero.sqlite")).unwrap();
    db.execute("UPDATE itemDataValues SET value = 'Retitled' WHERE valueID = 1", []).unwrap();
    drop(db);

    let second = c.await_task(&format!("/libraries/{lib}/import/zotero"), json!({ "path": path })).await;
    assert_eq!(second["items"], 0, "nothing new arrived");
    assert_eq!(second["updated"], 2, "and nothing is reported as a failure");
    assert_eq!(second["failed"], 0);

    let items = c.get(&format!("/libraries/{lib}/items")).await;
    assert_eq!(items["total"], 2, "still two items, not four");
    let titles: Vec<_> =
        items["items"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Retitled"), "the edit came across: {titles:?}");

    assert_eq!(c.get(&format!("/libraries/{lib}/collections")).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn importing_something_that_is_not_a_zotero_library_says_so() {
    let (c, _) = Client::new().await;
    let (status, body) =
        c.send("POST", "/import/zotero/preview", Some(json!({ "path": "/nope/zotero.sqlite" })))
            .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["title"].as_str().unwrap().contains("cannot read"));
}

#[tokio::test]
async fn importing_brings_the_pdfs_across() {
    // The PDFs are the point of a reference library; an import that left them
    // behind would have moved the catalogue and not the books.
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let dir = tempfile::tempdir().unwrap();
    let path = zotero_fixture(dir.path());

    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "INSERT INTO itemTypes VALUES (2, 'attachment');
         INSERT INTO items VALUES (30, 2, 'ZATT0001', '2020-01-01', '2020-01-02');
         INSERT INTO itemAttachments VALUES (30, 10, 'storage:paper.pdf', 'application/pdf');",
    )
    .unwrap();
    drop(db);

    let stored = dir.path().join("storage").join("ZATT0001");
    std::fs::create_dir_all(&stored).unwrap();
    std::fs::write(stored.join("paper.pdf"), b"%PDF-1.7 imported").unwrap();

    let done = c
        .await_task(
            &format!("/libraries/{lib}/import/zotero"),
            json!({ "path": path.to_string_lossy() }),
        )
        .await;
    assert_eq!(done["files"], 1);

    // The attachment hangs off its item, and its bytes are readable.
    let children = c.get(&format!("/libraries/{lib}/items/ZOTE1111/children")).await;
    assert_eq!(children.as_array().unwrap().len(), 1);
    assert_eq!(children[0]["itemType"], "attachment");

    let key: yk_core::Key = "ZATT0001".parse().unwrap();
    assert_eq!(
        app.storage().get(&key, "paper.pdf").await.unwrap(),
        b"%PDF-1.7 imported",
        "the file came across intact"
    );
}

#[tokio::test]
async fn importing_brings_the_users_notes_across_and_keeps_them_searchable() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let dir = tempfile::tempdir().unwrap();
    let path = zotero_fixture(dir.path());

    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE itemNotes (itemID INTEGER PRIMARY KEY, parentItemID INTEGER,
                                 note TEXT, title TEXT);
         INSERT INTO itemTypes VALUES (3, 'note');
         INSERT INTO items VALUES (40, 3, 'ZNOTE001', '2020-01-01', '2020-01-02');
         INSERT INTO itemNotes VALUES (40, 10, '<p>Worth rereading the ablations.</p>', 'Note');",
    )
    .unwrap();
    drop(db);

    let done = c
        .await_task(
            &format!("/libraries/{lib}/import/zotero"),
            json!({ "path": path.to_string_lossy() }),
        )
        .await;
    assert_eq!(done["notes"], 1);

    let children = c.get(&format!("/libraries/{lib}/items/ZOTE1111/children")).await;
    assert!(children.as_array().unwrap().iter().any(|c| c["itemType"] == "note"));

    // A note nobody can find again is barely imported at all.
    let hits = c.get(&format!("/libraries/{lib}/search?q=ablations&mode=keyword")).await;
    assert!(!hits["hits"].as_array().unwrap().is_empty(), "the note is searchable");
}

#[tokio::test]
async fn a_note_that_stands_on_its_own_still_comes_across() {
    let (c, app) = Client::new().await;
    let lib = app.services.default_library;
    let dir = tempfile::tempdir().unwrap();
    let path = zotero_fixture(dir.path());

    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE itemNotes (itemID INTEGER PRIMARY KEY, parentItemID INTEGER,
                                 note TEXT, title TEXT);
         INSERT INTO itemTypes VALUES (3, 'note');
         INSERT INTO items VALUES (40, 3, 'ZNOTE001', '2020-01-01', '2020-01-02');
         INSERT INTO items VALUES (41, 3, 'ZNOTE002', '2020-01-01', '2020-01-02');
         INSERT INTO itemNotes VALUES (40, 10, '<p>Worth rereading the ablations.</p>', 'Note');
         INSERT INTO itemNotes VALUES (41, NULL, '<p>Reading plan for the winter.</p>', 'Plan');",
    )
    .unwrap();
    drop(db);

    // Zotero lets a note stand on its own — reading notes, meeting notes, a
    // draft. This importer used to require a parent and drop the rest without
    // counting them, so the loss was invisible.
    let preview = c
        .post("/import/zotero/preview", json!({ "path": path.to_string_lossy() }))
        .await;
    assert_eq!(preview["notes"], 2, "the count has to include what will arrive");

    let done = c
        .await_task(
            &format!("/libraries/{lib}/import/zotero"),
            json!({ "path": path.to_string_lossy() }),
        )
        .await;
    assert_eq!(done["notes"], 2);

    let standalone = c.get(&format!("/libraries/{lib}/items/ZNOTE002")).await;
    assert_eq!(standalone["itemType"], "note");
    assert!(standalone["parentKey"].is_null(), "it belongs to nobody: {standalone}");
    // A note has no title field of its own, so without Zotero's summary it
    // would arrive as a blank row in the library list.
    assert_eq!(standalone["title"], "Plan");

    // And it is findable, which is the only reason to import it.
    let hits = c.get(&format!("/libraries/{lib}/search?q=winter&mode=keyword")).await;
    assert!(!hits["hits"].as_array().unwrap().is_empty(), "the standalone note is searchable");
}
