//! MCP protocol adapters over the same typed operations that register REST.

use axum::{
    body::{to_bytes, Body},
    http::Request,
    Router,
};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::{json, Value};
use std::sync::{Arc, Weak};
use tower::ServiceExt;
use yk_core::{Error, Result};

use crate::{
    operations::{Access, Arguments, Operation},
    state::{App, AppState},
};

pub const ENDPOINT: &str = "/api/v1/mcp";
const MAX_RESULT_BYTES: usize = 4 * 1024 * 1024;

pub struct Catalogue {
    router: Router<App>,
    entries: Vec<Entry>,
}

struct Entry {
    operation: Operation,
    validator: jsonschema::Validator,
}

impl Catalogue {
    pub fn new() -> Result<Self> {
        let api = crate::routes::api();
        let mut names = std::collections::HashSet::new();
        let mut entries = Vec::new();
        for operation in api.operations {
            if !names.insert(operation.name) {
                return Err(Error::internal(format!(
                    "duplicate MCP operation {}",
                    operation.name
                )));
            }
            let validator = jsonschema::validator_for(&operation.input_schema)
                .map_err(|e| Error::internal(format!("{} schema: {e}", operation.name)))?;
            entries.push(Entry {
                operation,
                validator,
            });
        }
        entries.sort_by_key(|entry| entry.operation.name);
        Ok(Self {
            router: api.router,
            entries,
        })
    }

    pub fn operations(&self) -> impl Iterator<Item = &Operation> {
        self.entries.iter().map(|entry| &entry.operation)
    }

    pub fn tools(&self, app: &App) -> Vec<Tool> {
        let config = app.config();
        self.operations()
            .filter(|op| permitted(op, &config))
            .map(|op| {
                Tool::new(
                    op.name,
                    op.description,
                    op.input_schema.as_object().expect("object schema").clone(),
                )
                .with_annotations(
                    ToolAnnotations::new()
                        .read_only(op.access == Access::Read)
                        .destructive(op.access != Access::Read),
                )
            })
            .collect()
    }

    pub async fn call(
        &self,
        app: App,
        library: i64,
        name: &str,
        arguments: Value,
    ) -> Result<Value> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.operation.name == name)
            .ok_or_else(|| Error::not_found(format!("no such MCP tool '{name}'")))?;
        let op = &entry.operation;
        if !permitted(op, &app.config()) {
            return Err(Error::invalid(format!(
                "operation '{name}' is disabled by MCP policy"
            )));
        }
        entry
            .validator
            .validate(&arguments)
            .map_err(|e| Error::invalid(format!("invalid arguments for {name}: {e}")))?;
        let args: Arguments = serde_json::from_value(arguments)?;
        let path_names: Vec<_> = op
            .path
            .split('/')
            .filter_map(|s| s.strip_prefix(':'))
            .collect();
        if args
            .path
            .keys()
            .any(|name| !path_names.contains(&name.as_str()))
        {
            return Err(Error::invalid("unknown path parameter"));
        }
        let mut url = url::Url::parse("http://localhost").expect("static URL");
        {
            let mut segments = url.path_segments_mut().expect("hierarchical URL");
            segments.clear();
            for segment in op.path.split('/').filter(|s| !s.is_empty()) {
                if let Some(name) = segment.strip_prefix(':') {
                    let value = match args.path.get(name) {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Number(n)) => n.to_string(),
                        None if name == "lib" => library.to_string(),
                        _ => return Err(Error::invalid(format!("missing path parameter {name}"))),
                    };
                    if value.is_empty() || value == "." || value == ".." {
                        return Err(Error::invalid(format!("invalid path parameter {name}")));
                    }
                    segments.push(&value);
                } else {
                    segments.push(segment);
                }
            }
        }
        if !args.query.is_empty() {
            let mut query = url.query_pairs_mut();
            for (key, value) in args.query {
                let value = match value {
                    Value::String(s) => s,
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => continue,
                    _ => return Err(Error::invalid(format!("query.{key} must be a scalar"))),
                };
                query.append_pair(&key, &value);
            }
        }
        let uri = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        };
        let mut request = Request::builder()
            .method(op.method.clone())
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(version) = args.if_version {
            request = request.header("If-Unmodified-Since-Version", version.to_string());
        }
        let body = args
            .body
            .map(|body| Body::from(body.to_string()))
            .unwrap_or_else(Body::empty);
        let response = self
            .router
            .clone()
            .with_state(app)
            .oneshot(
                request
                    .body(body)
                    .map_err(|e| Error::invalid(e.to_string()))?,
            )
            .await
            .map_err(|e| Error::internal(e.to_string()))?;
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = to_bytes(response.into_body(), MAX_RESULT_BYTES)
            .await
            .map_err(|e| {
                Error::Unavailable(format!(
                    "MCP response exceeds limit; narrow or page the request: {e}"
                ))
            })?;
        let value = if bytes.is_empty() {
            Value::Null
        } else if content_type.contains("json") {
            serde_json::from_slice(&bytes)?
        } else {
            json!({ "text": String::from_utf8(bytes.to_vec())
                .map_err(|_| Error::invalid("binary responses must be fetched using the file API"))?,
                "contentType": content_type })
        };
        if !status.is_success() {
            return Err(Error::invalid(format!("HTTP {}: {value}", status.as_u16())));
        }
        Ok(value)
    }
}

fn permitted(operation: &Operation, config: &crate::config::Config) -> bool {
    (operation.access != Access::Dangerous || config.mcp.allow_dangerous)
        && config.agent.tool_enabled(operation.name)
}

#[derive(Clone)]
pub struct McpServer {
    app: Weak<AppState>,
}

impl McpServer {
    pub fn new(app: &App) -> Self {
        Self {
            app: Arc::downgrade(app),
        }
    }

    fn app(&self) -> std::result::Result<App, ErrorData> {
        self.app
            .upgrade()
            .ok_or_else(|| ErrorData::internal_error("server stopped", None))
    }

    pub fn tools(&self) -> std::result::Result<Vec<Tool>, ErrorData> {
        let app = self.app()?;
        Ok(app.mcp.tools(&app))
    }

    pub async fn call(&self, library: Option<i64>, name: &str, arguments: Value) -> CallToolResult {
        let result = match self.app() {
            Ok(app) => {
                let catalogue = app.mcp.clone();
                catalogue
                    .call(
                        app.clone(),
                        library.unwrap_or(app.services.default_library),
                        name,
                        arguments,
                    )
                    .await
            }
            Err(error) => Err(Error::Unavailable(error.to_string())),
        };
        match result {
            Ok(value) => CallToolResult::structured(json!({ "result":value })),
            Err(error) => {
                tracing::warn!(tool = name, %error, "MCP operation failed");
                CallToolResult::error(vec![ContentBlock::text(error.to_string())])
            }
        }
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("yinkote", env!("CARGO_PKG_VERSION")))
            .with_instructions("Operations share the REST API. Arguments use path, query and body objects. path.lib defaults to the current library. Fetch item_schema for metadata fields. Ordinary edits are enabled; dangerous operations require explicit server configuration. Use pagination for large results.")
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|r| r.cursor.is_some()) {
            return Err(ErrorData::invalid_params("unknown cursor", None));
        }
        Ok(ListToolsResult::with_all_items(self.tools()?))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        Ok(self
            .call(
                None,
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
            )
            .await
            .into())
    }
}

pub fn router(app: App) -> Router<App> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };
    let server = McpServer::new(&app);
    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .enforce_origin_validation();
    // Preserve SDK rebinding checks locally; remote deployments are protected
    // by the application's API key guard and may use their own hostnames.
    if !crate::is_loopback(&app.config().host) {
        config = config.disable_allowed_hosts();
    }
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Router::new()
        .nest_service(ENDPOINT, service)
        .route(
            "/api/v1/mcp/settings",
            axum::routing::get(settings).put(configure),
        )
        .layer(axum::middleware::from_fn_with_state(
            app,
            crate::security::guard,
        ))
}

async fn settings(axum::extract::State(app): axum::extract::State<App>) -> axum::Json<Value> {
    axum::Json(json!({
        "endpoint": ENDPOINT, "allowDangerous": app.config().mcp.allow_dangerous,
        "tools":app.mcp.tools(&app).len(),
    }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsPatch {
    allow_dangerous: bool,
}

async fn configure(
    axum::extract::State(app): axum::extract::State<App>,
    axum::Json(patch): axum::Json<SettingsPatch>,
) -> crate::error::ApiResult<axum::Json<Value>> {
    let mut config = app.config();
    config.mcp.allow_dangerous = patch.allow_dangerous;
    config.save()?;
    *app.config.write() = config;
    Ok(settings(axum::extract::State(app)).await)
}

pub struct AgentSource {
    pub server: McpServer,
}

#[async_trait::async_trait]
impl yk_ai::ToolSource for AgentSource {
    fn names(&self) -> Vec<String> {
        match self.server.tools() {
            Ok(tools) => tools
                .into_iter()
                .map(|tool| tool.name.into_owned())
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "MCP catalogue unavailable");
                Vec::new()
            }
        }
    }

    async fn tools(&self, library_id: i64) -> Result<Vec<Arc<dyn yk_ai::Tool>>> {
        use rmcp::ServiceExt;
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = self.server.clone();
        tokio::spawn(async move {
            match server.serve(server_io).await {
                Ok(service) => {
                    if let Err(error) = service.waiting().await {
                        tracing::warn!(%error, "internal MCP connection ended");
                    }
                }
                Err(error) => tracing::warn!(%error, "internal MCP initialization failed"),
            }
        });
        let client = Arc::new(
            ().serve(client_io)
                .await
                .map_err(|e| Error::Unavailable(format!("MCP initialization: {e}")))?,
        );
        let definitions = client
            .peer()
            .list_all_tools()
            .await
            .map_err(|e| Error::Unavailable(format!("MCP discovery: {e}")))?;
        Ok(definitions
            .into_iter()
            .map(|definition| {
                Arc::new(AgentTool {
                    definition,
                    client: client.clone(),
                    library_id,
                }) as Arc<dyn yk_ai::Tool>
            })
            .collect())
    }
}

struct AgentTool {
    definition: Tool,
    client: Arc<rmcp::service::RunningService<rmcp::RoleClient, ()>>,
    library_id: i64,
}

#[async_trait::async_trait]
impl yk_ai::Tool for AgentTool {
    fn spec(&self) -> yk_ai::ToolSpec {
        yk_ai::ToolSpec {
            name: self.definition.name.to_string(),
            description: self.definition.description.as_deref().unwrap_or("").into(),
            parameters: Value::Object((*self.definition.input_schema).clone()),
        }
    }

    async fn call(&self, _: i64, arguments: Value) -> Result<Value> {
        let mut arguments = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| Error::invalid("tool arguments must be an object"))?;
        if self.definition.input_schema["properties"]["path"]["properties"]
            .get("lib")
            .is_some()
        {
            let path = arguments.entry("path").or_insert_with(|| json!({}));
            let path = path
                .as_object_mut()
                .ok_or_else(|| Error::invalid("path must be an object"))?;
            if path
                .get("lib")
                .is_some_and(|lib| lib.as_i64() != Some(self.library_id))
            {
                return Err(Error::invalid(
                    "agent tools are scoped to the current library",
                ));
            }
            path.insert("lib".into(), json!(self.library_id));
        }
        let result = self
            .client
            .peer()
            .call_tool(
                CallToolRequestParams::new(self.definition.name.clone()).with_arguments(arguments),
            )
            .await
            .map_err(|e| Error::Unavailable(format!("MCP tool call: {e}")))?;
        if result.is_error == Some(true) {
            return Err(Error::Unavailable(serde_json::to_string(&result.content)?));
        }
        result
            .structured_content
            .and_then(|v| v.get("result").cloned())
            .ok_or_else(|| Error::internal("MCP tool returned no structured result"))
    }
}

/// Stdio forwards protocol messages, never opens a second Store.
pub async fn stdio(config: &crate::config::Config) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let holder = crate::lock::holder(&config.data_dir()).ok_or_else(|| {
        anyhow::anyhow!(
            "no Yinkote server is running for {}; start it before yinkote mcp",
            config.data_dir().display()
        )
    })?;
    let endpoint = format!("{}{ENDPOINT}", holder.url());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let mut input = tokio::io::BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    loop {
        let mut line = Vec::new();
        let read = tokio::io::AsyncReadExt::take(&mut input, (MAX_RESULT_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .await?;
        if read == 0 {
            break;
        }
        anyhow::ensure!(line.len() <= MAX_RESULT_BYTES, "MCP input exceeds 4 MiB");
        let message: Value = serde_json::from_slice(&line)?;
        let mut request = client
            .post(&endpoint)
            .header("accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25")
            .json(&message);
        if let Some(key) = &config.api_key {
            request = request.bearer_auth(key);
        }
        let mut response = request.send().await?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= MAX_RESULT_BYTES,
                "MCP response exceeds 4 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        anyhow::ensure!(
            status.is_success(),
            "MCP server returned {status}: {}",
            String::from_utf8_lossy(&bytes)
        );
        if !bytes.is_empty() {
            let response: Value = serde_json::from_slice(&bytes)?;
            output.write_all(response.to_string().as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}
