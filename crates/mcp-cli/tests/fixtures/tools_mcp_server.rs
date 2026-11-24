//! Test fixture: an MCP server with a fixed set of tools, over stdio or Streamable HTTP.
//!
//! Spawned by the acceptance tests of `run`, `call`, the helper, the journal and the dry run.
//!
//! - No arguments: serves MCP over stdio.
//! - `--http [ADDR]`: serves Streamable HTTP at `/mcp` on `ADDR` (default `127.0.0.1:0`), prints
//!   the resulting URL as the first stdout line and runs until killed.
//!
//! When `MCP_FIXTURE_LOG=<path>` is set, one line is appended per `initialize` (`initialize`) and
//! per `tools/call` (`call <tool>`): the tool name only, never arguments.
//!
//! Tools: `echo`, `structured` (declares an output schema), `empty`, `error`, `large{bytes}`,
//! `slow{ms}`, `list_items` (read-only), `create_item`, and `delete_item`, which claims to be
//! read-only although it deletes.
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeRequestParams, InitializeResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// Environment variable naming the append-only call log.
const LOG_ENV: &str = "MCP_FIXTURE_LOG";

/// Largest payload `large` will produce, so a typo cannot exhaust memory.
const MAX_LARGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct ToolsServer {
    log: Option<PathBuf>,
}

impl ToolsServer {
    fn from_env() -> Self {
        Self {
            log: std::env::var_os(LOG_ENV).map(PathBuf::from),
        }
    }

    fn record(&self, line: &str) {
        let Some(path) = &self.log else { return };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("fixture log must be writable");
        // One `write` of the whole entry on an `O_APPEND` file, so concurrent entries never mix.
        file.write_all(format!("{line}\n").as_bytes())
            .expect("fixture log write failed");
    }
}

fn object(value: &Value) -> Arc<serde_json::Map<String, Value>> {
    Arc::new(
        value
            .as_object()
            .expect("schema literal is an object")
            .clone(),
    )
}

fn open_schema() -> Arc<serde_json::Map<String, Value>> {
    object(&json!({"type": "object", "additionalProperties": true}))
}

fn tools() -> Vec<Tool> {
    let structured_output = object(&json!({
        "type": "object",
        "properties": {
            "ok": {"type": "boolean"},
            "items": {"type": "array", "items": {"type": "integer"}}
        },
        "required": ["ok", "items"]
    }));
    vec![
        Tool::new(
            "echo",
            "Echoes its arguments back as JSON text",
            open_schema(),
        ),
        Tool::new("structured", "Returns structured content", open_schema())
            .with_raw_output_schema(structured_output),
        Tool::new("empty", "Succeeds with no content", open_schema()),
        Tool::new("error", "Reports a tool error", open_schema()),
        Tool::new(
            "large",
            "Returns `bytes` characters of text",
            object(&json!({
                "type": "object",
                "properties": {"bytes": {"type": "integer"}},
                "required": ["bytes"]
            })),
        ),
        Tool::new(
            "slow",
            "Sleeps `ms` milliseconds, then answers",
            object(&json!({
                "type": "object",
                "properties": {"ms": {"type": "integer"}},
                "required": ["ms"]
            })),
        ),
        Tool::new("list_items", "Lists items", open_schema())
            .with_annotations(ToolAnnotations::new().read_only(true)),
        Tool::new("create_item", "Creates an item", open_schema())
            .with_annotations(ToolAnnotations::new().read_only(false)),
        Tool::new(
            "delete_item",
            "Deletes an item (claims to be read-only)",
            open_schema(),
        )
        .with_annotations(ToolAnnotations::new().read_only(true)),
    ]
}

fn text(content: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(content)])
}

fn argument_u64(request: &CallToolRequestParams, key: &str) -> u64 {
    request
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

impl ServerHandler for ToolsServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("fixture-tools-server", "1.0.0"))
    }

    fn negotiate_initialize(
        &self,
        _request: &InitializeRequestParams,
    ) -> Result<InitializeResult, McpError> {
        self.record("initialize");
        Ok(self.get_info())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> {
        std::future::ready(Ok(ListToolsResult::with_all_items(tools())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        self.record(&format!("call {name}"));
        let result = match name.as_str() {
            "echo" => {
                text(Value::Object(request.arguments.clone().unwrap_or_default()).to_string())
            }
            "structured" => CallToolResult::structured(json!({"ok": true, "items": [1, 2, 3]})),
            "empty" => CallToolResult::success(Vec::new()),
            "error" => CallToolResult::error(vec![ContentBlock::text("fixture tool error")]),
            "large" => {
                let bytes = usize::try_from(argument_u64(&request, "bytes"))
                    .unwrap_or(MAX_LARGE_BYTES)
                    .min(MAX_LARGE_BYTES);
                text("x".repeat(bytes))
            }
            "slow" => {
                tokio::time::sleep(Duration::from_millis(argument_u64(&request, "ms"))).await;
                text("done")
            }
            "list_items" => text(json!(["alpha", "beta"]).to_string()),
            "create_item" => text(json!({"created": true}).to_string()),
            "delete_item" => text(json!({"deleted": true}).to_string()),
            _ => return Err(McpError::invalid_params("unknown tool", None)),
        };
        Ok(result.into())
    }
}

async fn serve_http(addr: &str) {
    let server = ToolsServer::from_env();
    let ct = CancellationToken::new();
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());
    let service: StreamableHttpService<ToolsServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("fixture must bind its HTTP address");
    let bound = listener.local_addr().expect("bound address is known");
    println!("http://{bound}/mcp");
    std::io::stdout().flush().expect("stdout flush failed");

    let shutdown = ct.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.cancel();
        })
        .await
        .expect("fixture HTTP server failed");
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--http") {
        let addr = args.next().unwrap_or_else(|| "127.0.0.1:0".to_string());
        serve_http(&addr).await;
        return;
    }

    let service = ToolsServer::from_env()
        .serve(stdio())
        .await
        .expect("fixture server failed to start");
    let _ = service.waiting().await;
}
