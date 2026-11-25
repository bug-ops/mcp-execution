//! Acceptance tests of per-tool drift (#688): `generate` records a digest per tool and `drift`
//! names the tools whose definition changed on the server.
//!
//! The live server is a small Node.js MCP server that lists whatever `tools.json` holds, so a
//! test can change one schema, or list a name twice, between `generate` and `drift`.

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use common::{Out, Sandbox, require_node};
use serde_json::{Value, json};

const SERVER: &str = "mutable";
const HEADING: &str = "changed on the server since generation";
const UNDETERMINED: &str = "cannot be determined";

const FAKE_SERVER: &str = r"const fs = require('node:fs');
const readline = require('node:readline');
const tools = process.argv[2];
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.id === undefined) return;
  const reply = (body) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: message.id, ...body }) + '\n');
  if (message.method === 'initialize') {
    reply({ result: { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'mutable', version: '1' } } });
  } else if (message.method === 'tools/list') {
    reply({ result: { tools: JSON.parse(fs.readFileSync(tools, 'utf8')) } });
  } else {
    reply({ error: { code: -32601, message: 'no such method' } });
  }
});
";

fn tool(name: &str, property_type: &str) -> Value {
    json!({
        "name": name,
        "description": format!("Does {name}"),
        "inputSchema": { "type": "object", "properties": { "value": { "type": property_type } } }
    })
}

fn set_tools(sb: &Sandbox, tools: &[Value]) {
    std::fs::write(sb.root.join("tools.json"), json!(tools).to_string()).unwrap();
}

/// A sandbox whose `mutable` server was generated with the tools `alpha` and `beta`.
fn generated(test: &str) -> Option<Sandbox> {
    if !require_node(test) {
        return None;
    }
    let sb = Sandbox::new();
    let script = sb.script("server.cjs", FAKE_SERVER);
    sb.write_mcp_json(&json!({ SERVER: {
        "command": "node",
        "args": [script.display().to_string(), sb.root.join("tools.json").display().to_string()]
    }}));
    set_tools(&sb, &[tool("alpha", "string"), tool("beta", "string")]);
    sb.generate_server(SERVER);
    Some(sb)
}

fn drift(sb: &Sandbox) -> Out {
    sb.cli(&["drift", "--from-config", SERVER])
}

fn drift_json(sb: &Sandbox) -> (Option<i32>, Value) {
    let out = sb.cli(&["--format", "json", "drift", "--from-config", SERVER]);
    let report = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {} {}", out.stdout, out.stderr));
    (out.code, report)
}

#[test]
fn generate_records_a_distinct_digest_for_every_tool() {
    let Some(sb) = generated("generate_records_a_distinct_digest_for_every_tool") else {
        return;
    };

    let meta: Value = serde_json::from_str(
        &std::fs::read_to_string(sb.servers.join(SERVER).join("_meta.json")).unwrap(),
    )
    .unwrap();

    assert_eq!(meta["schema_version"], 3);
    let digests: std::collections::BTreeSet<&str> = meta["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            tool["digest"]
                .as_str()
                .unwrap_or_else(|| panic!("no digest: {tool}"))
        })
        .collect();
    assert_eq!(digests.len(), 2, "{digests:?}");
    assert!(
        digests
            .iter()
            .all(|d| d.len() == 64 && d.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))),
        "{digests:?}"
    );
}

#[test]
fn drift_names_the_tool_whose_schema_changed_in_text_and_exits_5() {
    let Some(sb) = generated("drift_names_the_tool_whose_schema_changed_in_text") else {
        return;
    };
    set_tools(&sb, &[tool("alpha", "number"), tool("beta", "string")]);

    let out = drift(&sb);

    assert_eq!(out.code, Some(5), "{} {}", out.stdout, out.stderr);
    assert!(out.stdout.contains(HEADING), "{}", out.stdout);
    assert!(out.stdout.contains("alpha"), "{}", out.stdout);
    assert!(!out.stdout.contains("beta"), "{}", out.stdout);
    assert!(!out.stdout.contains(UNDETERMINED), "{}", out.stdout);
}

#[test]
fn drift_json_lists_only_the_changed_tool_under_changed_on_server() {
    let Some(sb) = generated("drift_json_lists_only_the_changed_tool") else {
        return;
    };
    set_tools(&sb, &[tool("alpha", "number"), tool("beta", "string")]);

    let (code, report) = drift_json(&sb);

    assert_eq!(code, Some(5), "{report}");
    assert_eq!(
        report["changed_on_server"],
        json!({ "names": ["alpha"], "more": 0 }),
        "{report}"
    );
    assert_eq!(report["only_on_server"]["names"], json!([]), "{report}");
    assert_eq!(report["only_recorded"]["names"], json!([]), "{report}");
}

#[test]
fn drift_lists_a_live_duplicate_with_a_differing_definition_as_changed() {
    let Some(sb) = generated("drift_lists_a_live_duplicate_with_a_differing_definition") else {
        return;
    };
    set_tools(
        &sb,
        &[
            tool("alpha", "string"),
            tool("alpha", "number"),
            tool("beta", "string"),
        ],
    );

    let (code, report) = drift_json(&sb);

    assert_eq!(code, Some(5), "{report}");
    assert_eq!(
        report["changed_on_server"]["names"],
        json!(["alpha"]),
        "{report}"
    );
}

#[test]
fn drift_keeps_the_undetermined_notice_for_metadata_without_tool_digests() {
    let Some(sb) = generated("drift_keeps_the_undetermined_notice_for_metadata_without_digests")
    else {
        return;
    };
    sb.edit_meta(SERVER, |meta| {
        for tool in meta["tools"].as_array_mut().unwrap() {
            tool.as_object_mut().unwrap().remove("digest");
        }
    });
    set_tools(&sb, &[tool("alpha", "number"), tool("beta", "string")]);

    let (code, report) = drift_json(&sb);

    assert_eq!(code, Some(5), "{report}");
    assert_eq!(report["changed_on_server"]["names"], json!([]), "{report}");
    assert!(
        report["notices"].to_string().contains(UNDETERMINED),
        "{report}"
    );
}
