//! Acceptance test for #565: pathologically deep schemas of every nesting kind degrade instead
//! of overflowing the stack.

use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
use serde_json::Value;

const DEPTH: usize = 5_000;
const SMALL_STACK_BYTES: usize = 1024 * 1024;

fn obj(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

fn nest(kind: &str) -> Value {
    let mut schema = obj(vec![("type", Value::from("string"))]);
    for _ in 0..DEPTH {
        schema = match kind {
            "object" => obj(vec![
                ("type", Value::from("object")),
                ("properties", obj(vec![("a", schema)])),
            ]),
            "array" => obj(vec![("type", Value::from("array")), ("items", schema)]),
            "all_of" => obj(vec![("allOf", Value::Array(vec![schema]))]),
            "one_of" => obj(vec![(
                "oneOf",
                Value::Array(vec![schema, obj(vec![("type", Value::from("number"))])]),
            )]),
            _ => obj(vec![
                ("type", Value::from("object")),
                ("additionalProperties", schema),
            ]),
        };
    }
    schema
}

fn on_small_stack<T: Send + 'static>(run: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK_BYTES)
        .spawn(run)
        .unwrap()
        .join()
        .expect("conversion must not overflow the stack or panic")
}

#[test]
fn convert_schema_deeply_nested_schemas_of_every_kind_degrade_without_overflow() {
    for kind in ["object", "array", "all_of", "one_of", "additional"] {
        let (compact, declaration) = on_small_stack(move || {
            let schema = nest(kind);
            let converted = convert_schema(&schema);
            let rendered = (
                converted.render(RenderStyle::Compact),
                converted.render(RenderStyle::Declaration),
            );
            // Dropping a 5,000-deep `Value` is itself recursive and not what is under test.
            std::mem::forget(schema);
            rendered
        });

        assert!(
            compact.contains("unknown"),
            "{kind}: the cut-off node is unknown"
        );
        assert!(!compact.contains('\n'), "{kind}");
        assert!(
            declaration.len() <= 64 * 1024 + 64,
            "{kind}: {} B",
            declaration.len()
        );
    }
}
