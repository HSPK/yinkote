use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;
use yk_ai::ToolSource;
use yk_core::{event::DomainEvent, model::ItemDraft};
use yk_server::{
    config::Config,
    mcp::{AgentSource, McpServer, ENDPOINT},
    state::App,
};
use yk_store::Store;

async fn application() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        data_dir: Some(dir.path().to_path_buf()),
        ..Default::default()
    };
    let app = yk_server::build_with_store(config, Store::in_memory().unwrap())
        .await
        .unwrap();
    (dir, app)
}

async fn rpc(app: &App, message: Value, headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(ENDPOINT)
        .header("host", "localhost:23130")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = yk_server::router(app.clone())
        .oneshot(request.body(Body::from(message.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8(bytes.to_vec()).unwrap()))
    };
    (status, value)
}

fn list_request() -> Value {
    json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
}

#[tokio::test]
async fn streamable_http_initializes_and_discovers_schema_driven_operations() {
    let (_dir, app) = application().await;
    let (status, response) = rpc(
        &app,
        json!({
            "jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-11-25","capabilities":{},
                "clientInfo":{"name":"test-client","version":"1"}
            }
        }),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["result"]["serverInfo"]["name"], "yinkote");
    let (status, listed) = rpc(&app, list_request(), &[]).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 50, "{listed}");
    let create = tools
        .iter()
        .find(|tool| tool["name"] == "create_items")
        .unwrap();
    assert!(create["inputSchema"]["properties"]["body"]["anyOf"].is_array());
    let patch = tools
        .iter()
        .find(|tool| tool["name"] == "update_item")
        .unwrap();
    assert_eq!(
        patch["inputSchema"]["properties"]["path"]["properties"]["lib"]["type"],
        "integer"
    );
    assert!(patch["inputSchema"]["properties"]["body"]["properties"]["fields"].is_object());
    assert!(tools.iter().all(|tool| tool["name"] != "delete_items"));
    assert!(tools.iter().all(|tool| tool["name"] != "configure_mcp"));
    let (status, _) = rpc(
        &app,
        json!({
            "jsonrpc":"2.0","method":"notifications/initialized"
        }),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, ping) = rpc(&app, json!({"jsonrpc":"2.0","id":4,"method":"ping"}), &[]).await;
    assert_eq!(ping["id"], 4);
}

#[tokio::test]
async fn http_auth_and_browser_origin_checks_cover_mcp_not_just_rest() {
    let (_dir, app) = application().await;
    app.config.write().api_key = Some("test-key".into());
    assert_eq!(
        rpc(&app, list_request(), &[]).await.0,
        StatusCode::UNAUTHORIZED
    );
    let valid = [("authorization", "Bearer test-key")];
    assert_eq!(rpc(&app, list_request(), &valid).await.0, StatusCode::OK);
    assert_eq!(
        rpc(&app, list_request(), &[("authorization", "Bearer wrong")])
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let forbidden = rpc(
        &app,
        list_request(),
        &[
            ("authorization", "Bearer test-key"),
            ("origin", "https://example.invalid"),
        ],
    )
    .await
    .0;
    assert_eq!(forbidden, StatusCode::FORBIDDEN);
    app.config.write().host = "localhost".into();
    let response = yk_server::router(app.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(ENDPOINT)
                .header("host", "attacker.invalid")
                .header("authorization", "Bearer test-key")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(list_request().to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn mcp_mutations_share_rest_events_versions_and_error_behavior() {
    let (_dir, app) = application().await;
    let mut events = app.events().subscribe();
    let (status, created) = rpc(&app, json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"create_items", "arguments":{"body":{"itemType":"journalArticle","title":"MCP paper","date":"2024"}}
        }
    }), &[]).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["result"]["isError"], false, "{created}");
    let page = app
        .store()
        .items
        .list(&yk_core::query::ItemQuery {
            filter: yk_core::query::ItemFilter {
                library_id: 1,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    let paper = &page.items[0];
    assert!(matches!(
        events.try_recv().unwrap(),
        DomainEvent::ItemsChanged { .. }
    ));
    let update = json!({"path":{"key":paper.key},"body":{"fields":{"title":"Updated"}},"ifVersion":paper.version});
    app.mcp
        .call(app.clone(), 1, "update_item", update.clone())
        .await
        .unwrap();
    let conflict = app
        .mcp
        .call(app.clone(), 1, "update_item", update)
        .await
        .unwrap_err();
    assert!(conflict.to_string().contains("412"), "{conflict}");
    assert!(app
        .mcp
        .call(
            app.clone(),
            1,
            "update_item",
            json!({"path":{"key":paper.key},"body":{"typo":"bad"}})
        )
        .await
        .is_err());
    assert!(app
        .mcp
        .call(
            app.clone(),
            1,
            "search_library",
            json!({"query":{"typo":"bad"}})
        )
        .await
        .is_err());
    assert!(app
        .mcp
        .call(
            app.clone(),
            1,
            "get_item",
            json!({"path":{"key":"../../settings"}})
        )
        .await
        .is_err());
}

#[tokio::test]
async fn dangerous_policy_is_enforced_at_execution_and_cannot_be_enabled_by_a_tool() {
    let (_dir, app) = application().await;
    let paper = app
        .store()
        .items
        .create(1, ItemDraft::new("book").with_field("title", "Keep"))
        .await
        .unwrap();
    let server = McpServer::new(&app);
    let args = json!({"body":{"keys":[paper.key]}});
    assert_eq!(
        server
            .call(None, "delete_items", args.clone())
            .await
            .is_error,
        Some(true)
    );
    assert_eq!(
        server
            .call(None, "configure_mcp", json!({"allowDangerous":true}))
            .await
            .is_error,
        Some(true)
    );
    app.config.write().mcp.allow_dangerous = true;
    assert!(server
        .tools()
        .unwrap()
        .iter()
        .any(|tool| tool.name == "delete_items"));
    app.config.write().mcp.allow_dangerous = false;
    assert_eq!(
        server
            .call(None, "delete_items", args.clone())
            .await
            .is_error,
        Some(true)
    );
    assert!(app.store().items.get(1, &paper.key).await.is_ok());
    app.config.write().mcp.allow_dangerous = true;
    assert_eq!(
        server.call(None, "delete_items", args).await.is_error,
        Some(false)
    );
    assert!(app.store().items.get(1, &paper.key).await.is_err());
}

#[tokio::test]
async fn the_assistant_discovers_and_calls_mcp_instead_of_a_second_business_implementation() {
    let (_dir, app) = application().await;
    let source = AgentSource {
        server: McpServer::new(&app),
    };
    let tools = source.tools(1).await.unwrap();
    assert!(tools.len() >= 50);
    let create = tools
        .iter()
        .find(|tool| tool.spec().name == "create_items")
        .unwrap();
    let value = create
        .call(
            1,
            json!({"body":{"itemType":"book","title":"Created through MCP"}}),
        )
        .await
        .unwrap();
    assert!(value.is_object());
    let search = tools
        .iter()
        .find(|tool| tool.spec().name == "search_library")
        .unwrap();
    let page = search
        .call(1, json!({"query":{"q":"Created","mode":"keyword"}}))
        .await
        .unwrap();
    assert_eq!(page["items"][0]["title"], "Created through MCP");
    assert!(search.call(1, json!({"path":{"lib":2}})).await.is_err());
    app.config
        .write()
        .agent
        .disabled_tools
        .push("create_items".into());
    assert!(create
        .call(1, json!({"body":{"itemType":"book","title":"Denied"}}))
        .await
        .is_err());
    assert!(!source.names().contains(&"create_items".to_string()));
}

struct Model(std::sync::atomic::AtomicUsize);

#[async_trait::async_trait]
impl yk_ai::ChatProvider for Model {
    fn model(&self) -> String {
        "scripted-mcp-model".into()
    }
    async fn complete(
        &self,
        request: yk_ai::ChatRequest,
        _: yk_ai::Sink<'_>,
    ) -> yk_core::Result<yk_ai::ChatMessage> {
        let create = request
            .tools
            .iter()
            .find(|tool| tool.name == "create_items")
            .expect("the model was not given the discovered catalogue");
        assert!(create.parameters["properties"]["body"].is_object());
        if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            Ok(yk_ai::ChatMessage {
                role: "assistant".into(),
                tool_calls: vec![yk_ai::ToolCall {
                    id: "call-1".into(),
                    name: "create_items".into(),
                    arguments: json!({"body":{"itemType":"book","title":"Model-created paper"}}),
                }],
                ..Default::default()
            })
        } else {
            assert_eq!(request.messages.last().unwrap().role, "tool");
            Ok(yk_ai::ChatMessage::new("assistant", "Added the paper."))
        }
    }
}

#[tokio::test]
async fn a_complete_agent_turn_executes_discovered_mcp_tools() {
    let (_dir, app) = application().await;
    let source = std::sync::Arc::new(AgentSource {
        server: McpServer::new(&app),
    });
    let agent = yk_agent::Agent::new(
        std::sync::Arc::new(Model(std::sync::atomic::AtomicUsize::new(0))),
        Vec::new(),
        "Test".into(),
    )
    .with_tool_source(source);
    let turn = agent
        .run(1, vec![yk_ai::ChatMessage::new("user", "Add a paper")])
        .await
        .unwrap();
    assert_eq!(turn.reply, "Added the paper.");
    let found = app
        .mcp
        .call(
            app.clone(),
            1,
            "search_library",
            json!({"query":{"q":"Model-created","mode":"keyword"}}),
        )
        .await
        .unwrap();
    assert_eq!(found["items"][0]["title"], "Model-created paper");
}

#[tokio::test]
async fn batch_tagging_is_one_shared_operation_and_preserves_unrelated_tags() {
    let (_dir, app) = application().await;
    let first = app
        .store()
        .items
        .create(
            1,
            ItemDraft {
                tags: vec![yk_core::model::ItemTag::manual("keep")],
                ..ItemDraft::new("book").with_field("title", "One")
            },
        )
        .await
        .unwrap();
    let second = app
        .store()
        .items
        .create(1, ItemDraft::new("book").with_field("title", "Two"))
        .await
        .unwrap();
    let arguments = json!({"body":{"keys":[first.key,second.key],"tags":["added"]}});
    let result = app
        .mcp
        .call(app.clone(), 1, "tag_items", arguments.clone())
        .await
        .unwrap();
    assert_eq!(result["changed"], 2);
    let result = app
        .mcp
        .call(app.clone(), 1, "untag_items", arguments)
        .await
        .unwrap();
    assert_eq!(result["changed"], 2);
    assert_eq!(
        app.store().items.get(1, &first.key).await.unwrap().tags,
        vec![yk_core::model::ItemTag::manual("keep")]
    );
}

#[tokio::test]
async fn mcp_settings_work_without_a_configured_model_and_persist() {
    let (dir, app) = application().await;
    let response = yk_server::router(app.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/mcp/settings")
                .header("host", "localhost")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"allowDangerous":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(app.config().mcp.allow_dangerous);
    let stored = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
    let parsed: Config = toml::from_str(&stored).unwrap();
    assert!(parsed.mcp.allow_dangerous);
    assert!(app.agent().is_none());
}

#[tokio::test]
async fn stdio_uses_the_existing_server_and_keeps_stdout_protocol_only() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (dir, app) = application().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let _lock = yk_server::lock::acquire(dir.path(), port, "127.0.0.1").unwrap();
    let router = yk_server::router(app);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_yinkote"))
        .args(["mcp", "--data-dir"])
        .arg(dir.path())
        .env_remove("YK_API_KEY")
        .env_remove("YK_DATA_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"stdio-test","version":"1"}
    }});
    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let line = tokio::time::timeout(std::time::Duration::from_secs(10), stdout.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["result"]["serverInfo"]["name"], "yinkote");
    stdin
        .write_all(format!("{}\n", list_request()).as_bytes())
        .await
        .unwrap();
    let line = tokio::time::timeout(std::time::Duration::from_secs(10), stdout.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert!(response["result"]["tools"].as_array().unwrap().len() >= 50);
    drop(stdin);
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    server.abort();
    assert!(status.success());
    assert!(
        !dir.path().join("yinkote.db").exists(),
        "stdio opened another library"
    );
}
