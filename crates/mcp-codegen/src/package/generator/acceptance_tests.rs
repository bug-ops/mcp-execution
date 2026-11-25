//! Acceptance tests for #482, #568 and the provable per-tool bytes bound (S2, S3): degrade
//! behaviour, the stored-text cap, compact `_meta.json` and the oversize warning, all observed
//! through `generate`.

use super::*;
use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
use mcp_execution_core::metadata::{
    MAX_METADATA_FILE_BYTES, METADATA_FILE_NAME, ResultTypeOrigin, ServerMetadata,
};
use mcp_execution_core::untrusted::MAX_UNTRUSTED_FIELD_LEN;
use mcp_execution_core::{ServerId, ToolName};
use mcp_execution_introspector::{
    MAX_SCHEMA_SIZE_BYTES, MAX_TOOL_COUNT, MAX_TOOL_DESCRIPTION_LEN, MAX_TOOL_NAME_LEN,
    ServerCapabilities,
};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, span};

struct WarnCapture(Arc<Mutex<Vec<String>>>);

struct FieldText(String);

impl Visit for FieldText {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl tracing::Subscriber for WarnCapture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::WARN
    }

    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        if *event.metadata().level() == Level::WARN {
            let mut text = FieldText(String::new());
            event.record(&mut text);
            self.0.lock().unwrap().push(text.0);
        }
    }

    fn enter(&self, _: &span::Id) {}

    fn exit(&self, _: &span::Id) {}
}

fn capture_warnings<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let value = tracing::subscriber::with_default(WarnCapture(Arc::clone(&warnings)), run);
    let collected = warnings.lock().unwrap().clone();
    (value, collected)
}

fn config() -> ServerConfig {
    ServerConfig::builder()
        .command("test-command".to_string())
        .build()
        .unwrap()
}

fn tool(name: &str, input: Value, output: Option<Value>) -> ToolInfo {
    {
        let mut value = ToolInfo::new(ToolName::new(name).unwrap(), format!("Tool {name}"), input);
        value.output_schema = output;
        value
    }
}

fn info(tools: Vec<ToolInfo>) -> ServerInfo {
    ServerInfo::new(
        ServerId::new("acme").unwrap(),
        "Acme".to_string(),
        "1.0.0".to_string(),
        tools,
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    )
}

fn generate(server: &ServerInfo) -> GeneratedCode {
    PackageGenerator::new()
        .unwrap()
        .generate(server, &config())
        .expect("generation succeeds")
}

fn file<'a>(code: &'a GeneratedCode, path: &str) -> &'a str {
    &code
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} not generated"))
        .content
}

fn metadata(code: &GeneratedCode) -> ServerMetadata {
    serde_json::from_str(file(code, METADATA_FILE_NAME)).expect("sidecar parses")
}

fn object_schema_with_properties(prefix: &str, target_bytes: usize) -> Value {
    let mut properties = serde_json::Map::new();
    let mut index = 0;
    while serde_json::to_vec(&properties).unwrap().len() < target_bytes {
        properties.insert(format!("{prefix}{index:05}"), json!({"type": "string"}));
        index += 1;
    }
    json!({"type": "object", "properties": properties})
}

fn adversarial_tool() -> ToolInfo {
    let near_limit = MAX_SCHEMA_SIZE_BYTES - 4096;
    {
        let mut value = ToolInfo::new(
            ToolName::new("a".repeat(MAX_TOOL_NAME_LEN)).unwrap(),
            "\u{1}".repeat(MAX_TOOL_DESCRIPTION_LEN),
            object_schema_with_properties("in", near_limit),
        );
        value.output_schema = Some(object_schema_with_properties("out", near_limit));
        value.annotations = Some({
            let mut value = ToolAnnotations::default();
            value.read_only = Hint::StatedTrue;
            value.destructive = Hint::StatedFalse;
            value.idempotent = Hint::StatedTrue;
            value.open_world = Hint::StatedFalse;

            value.title = AnnotationTitle::new(&"T".repeat(MAX_UNTRUSTED_FIELD_LEN));
            value
        });
        value
    }
}

#[test]
fn generate_adversarial_tool_stays_within_the_per_tool_budget_and_warns() {
    let server = info(vec![adversarial_tool()]);

    let (code, warnings) = capture_warnings(|| generate(&server));

    let meta = metadata(&code);
    let tool_file = code
        .files
        .iter()
        .find(|f| {
            std::path::Path::new(&f.path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ts"))
                && !f.path.starts_with("_runtime/")
                && f.path != "index.ts"
                && f.path != "catalog.ts"
        })
        .expect("one tool file");
    let meta_entry = serde_json::to_string(&meta.tools[0]).unwrap().len() + 1;
    assert!(
        tool_file.content.len() + meta_entry <= PER_TOOL_RENDER_BUDGET,
        "tool file {} B + meta entry {meta_entry} B exceed the render budget {PER_TOOL_RENDER_BUDGET} B",
        tool_file.content.len()
    );
    let total: usize = code.files.iter().map(|f| f.content.len()).sum();
    assert!(
        total <= PER_TOOL_BYTES_MAX + FIXED_FILES_BYTES_MAX,
        "{total} B in total"
    );
    assert!(
        !warnings.is_empty(),
        "degrading a tool must be reported with a warning"
    );
}

#[test]
fn generate_declared_output_schema_is_stored_as_compact_result_type() {
    let server = info(vec![
        tool(
            "typed",
            json!({"type": "object"}),
            Some(
                json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}),
            ),
        ),
        tool("untyped", json!({"type": "object"}), None),
    ]);

    let meta = metadata(&generate(&server));
    let typed = meta
        .tools
        .iter()
        .find(|t| t.name.as_str() == "typed")
        .unwrap();
    let untyped = meta
        .tools
        .iter()
        .find(|t| t.name.as_str() == "untyped")
        .unwrap();

    let text = typed
        .result_type
        .as_deref()
        .expect("a small declared type is stored");
    assert!(text.contains("id") && text.contains("number"), "{text}");
    assert!(
        !text.contains('\n'),
        "stored text is the compact form: {text}"
    );
    assert_eq!(typed.result_type_origin, ResultTypeOrigin::Declared);
    assert_eq!(untyped.result_type, None);
    assert_eq!(untyped.result_type_origin, ResultTypeOrigin::Undeclared);
}

#[test]
fn generate_result_type_over_the_stored_cap_is_stored_as_absent_but_still_typed_in_the_file() {
    let wide = object_schema_with_properties("field", 4 * STORED_TYPE_TEXT_MAX_BYTES);
    let server = info(vec![tool("wide", json!({"type": "object"}), Some(wide))]);

    let code = generate(&server);
    let meta = metadata(&code);

    assert_eq!(meta.tools[0].result_type, None);
    assert_eq!(meta.tools[0].result_type_origin, ResultTypeOrigin::Declared);
    let tool_file = file(&code, &format!("{}.ts", meta.tools[0].typescript_name));
    assert!(
        tool_file.contains("field00000"),
        "the file keeps the full type"
    );
}

#[test]
fn generate_parameter_type_over_the_stored_cap_is_stored_as_unknown() {
    let values: Vec<String> = (0..80)
        .map(|i| format!("value-{i:03}-{}", "x".repeat(16)))
        .collect();
    let input = json!({
        "type": "object",
        "properties": {"mode": {"enum": values}, "q": {"type": "string"}},
        "required": ["mode", "q"]
    });
    let code = generate(&info(vec![tool("search", input, None)]));

    let meta = metadata(&code);
    let parameter = |name: &str| {
        meta.tools[0]
            .parameters
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("parameter {name} listed"))
    };
    assert_eq!(parameter("mode").typescript_type, "unknown");
    assert_eq!(parameter("q").typescript_type, "string");
    assert!(
        file(&code, "search.ts").contains("'value-079-"),
        "the file keeps the full enum"
    );
}

#[test]
fn generate_unrepresentable_output_schema_is_degraded_and_generation_succeeds() {
    let server = info(vec![tool(
        "odd",
        json!({"type": "object"}),
        Some(json!("not a schema")),
    )]);

    let code = generate(&server);
    let meta = metadata(&code);

    assert_eq!(meta.tools[0].result_type, None);
    assert_eq!(meta.tools[0].result_type_origin, ResultTypeOrigin::Degraded);
    let tool_file = file(&code, "odd.ts");
    assert!(
        tool_file.contains(
            "Record<string, unknown> | unknown[] | string | number | boolean | null | undefined"
        ),
        "a degraded declared result widens to every JSON value and stays visible as empty"
    );
    assert!(
        tool_file.contains("'structured')"),
        "the result source stays structured for a declared schema"
    );
}

fn typical_tools(count: usize) -> Vec<ToolInfo> {
    (0..count)
        .map(|i| {
            tool(
                &format!("tool_{i:04}"),
                json!({
                    "type": "object",
                    "properties": {
                        "repo": {"type": "string", "description": "Repository"},
                        "page": {"type": "integer"},
                        "state": {"enum": ["open", "closed"]}
                    },
                    "required": ["repo"]
                }),
                None,
            )
        })
        .collect()
}

#[test]
fn generate_with_the_maximum_tool_count_writes_a_compact_sidecar_under_the_scan_cap() {
    let code = generate(&info(typical_tools(MAX_TOOL_COUNT)));

    assert_eq!(code.file_count(), MAX_TOOL_COUNT + FIXED_FILE_COUNT);
    let sidecar = file(&code, METADATA_FILE_NAME);
    assert_eq!(
        sidecar.trim_end().lines().count(),
        1,
        "_meta.json is compact JSON"
    );
    assert!(
        u64::try_from(sidecar.len()).unwrap() <= MAX_METADATA_FILE_BYTES,
        "{} B",
        sidecar.len()
    );
}

#[test]
fn generate_sidecar_over_the_scan_cap_still_succeeds_and_warns_with_size_and_cap() {
    let wide_input = object_schema_with_properties("param_", 3_000);
    let tools: Vec<ToolInfo> = (0..500)
        .map(|i| tool(&format!("tool_{i:04}"), wide_input.clone(), None))
        .collect();
    let server = info(tools);

    let (code, warnings) = capture_warnings(|| generate(&server));

    let size = file(&code, METADATA_FILE_NAME).len();
    assert!(
        u64::try_from(size).unwrap() > MAX_METADATA_FILE_BYTES,
        "fixture must exceed the cap, got {size} B"
    );
    let size_text = size.to_string();
    let cap_text = MAX_METADATA_FILE_BYTES.to_string();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains(&size_text) && w.contains(&cap_text)),
        "expected one warning naming the size {size_text} and cap {cap_text}: {warnings:?}"
    );
}
