//! End-to-end tests of the generated bridge against the real helper (#575): `run` hands the
//! script the helper channel (fd 0 socketpair on Unix, named pipe on Windows), and a generated
//! tool call travels bridge -> channel -> helper -> fixture server and back.
//!
//! Requires the `test-fixtures` feature and Node.js 22.18+. The tests run on every platform, so
//! the Windows named pipe is covered in CI.

#![cfg(feature = "test-fixtures")]

mod common;
mod package;

use common::{Out, Sandbox, require_node};

/// Server id no `mcp.json` is expected to define, so the helper must answer with a failure.
const ABSENT_SERVER: &str = "e2e-absent-server";

/// Result size just under the helper line bound, once the response envelope is added.
const NEAR_LIMIT: usize = 4_100_000;

fn sandbox(test: &str) -> Option<Sandbox> {
    require_node(test).then(Sandbox::new)
}

fn run_script_text(sb: &Sandbox, name: &str, body: &str) -> Out {
    let script = sb.script(name, body);
    sb.run_script(&script, &["--timeout", "60"])
}

#[test]
fn helper_failure_reaches_the_script_as_a_typed_error_and_the_channel_survives() {
    let Some(sb) = sandbox("helper_failure_reaches_the_script_as_a_typed_error") else {
        return;
    };
    package::write_package(&sb.servers, ABSENT_SERVER, "Offline", &["ping"]);

    let out = run_script_text(
        &sb,
        "absent.ts",
        &format!(
            "import {{ ping }} from 'mcp:{ABSENT_SERVER}';\n\
             for (let attempt = 0; attempt < 2; attempt++) {{\n\
               try {{ await ping({{}}); console.log('UNEXPECTED'); }}\n\
               catch (e: any) {{\n\
                 console.log('KIND=' + e.kind + ' CLOSED=' + e.message.includes('helper channel closed'));\n\
               }}\n\
             }}\n"
        ),
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let kinds: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("KIND="))
        .collect();
    assert_eq!(
        kinds, ["KIND=call_failed CLOSED=false"; 2],
        "a failed call must not take the channel down: {}",
        out.stdout
    );
}

#[test]
fn concurrent_calls_share_one_server_session() {
    let Some(sb) = sandbox("concurrent_calls_share_one_server_session") else {
        return;
    };
    sb.generate();
    sb.clear_fixture_log();

    let out = run_script_text(
        &sb,
        "concurrent.ts",
        "import { echo } from 'mcp:fixture';\n\
         const all = await Promise.all([1, 2, 3].map((n) => echo({ n })));\n\
         console.log('RESULTS=' + JSON.stringify(all));\n",
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#"RESULTS=[{"n":1},{"n":2},{"n":3}]"#),
        "{}",
        out.stdout
    );
    // The fixture appends its entries without taking a lock, so concurrent calls can run two
    // entries together on one line; count occurrences instead of comparing lines.
    let log = sb.fixture_log().join("\n");
    assert_eq!(log.matches("initialize").count(), 1, "one session: {log:?}");
    assert_eq!(log.matches("call echo").count(), 3, "three calls: {log:?}");
}

#[test]
fn tool_error_and_empty_success_keep_their_shapes_through_the_helper() {
    let Some(sb) = sandbox("tool_error_and_empty_success_keep_their_shapes_through_the_helper")
    else {
        return;
    };
    sb.generate();

    let out = run_script_text(
        &sb,
        "shapes.ts",
        "import { error, empty } from 'mcp:fixture';\n\
         console.log('EMPTY=' + ((await empty({})) === undefined));\n\
         try { await error({}); console.log('UNEXPECTED'); }\n\
         catch (e: any) {\n\
           console.log('KIND=' + e.kind + ' SERVER=' + e.serverMessage + ' IN_MESSAGE=' + e.message.includes('fixture tool error'));\n\
         }\n",
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("EMPTY=true"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("KIND=tool_error SERVER=fixture tool error IN_MESSAGE=false"),
        "server text lives only in serverMessage: {}",
        out.stdout
    );
}

#[test]
fn a_result_just_under_the_line_bound_arrives_complete() {
    let Some(sb) = sandbox("a_result_just_under_the_line_bound_arrives_complete") else {
        return;
    };
    sb.generate();

    let out = run_script_text(
        &sb,
        "large.ts",
        &format!(
            "import {{ large }} from 'mcp:fixture';\n\
             const text = (await large({{ bytes: {NEAR_LIMIT} }})) as string;\n\
             console.log('LENGTH=' + text.length);\n"
        ),
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("LENGTH={NEAR_LIMIT}")),
        "{}",
        out.stdout
    );
}

#[test]
fn a_result_over_the_line_bound_fails_the_call_without_hanging_the_script() {
    let Some(sb) =
        sandbox("a_result_over_the_line_bound_fails_the_call_without_hanging_the_script")
    else {
        return;
    };
    sb.generate();

    let out = run_script_text(
        &sb,
        "too-large.ts",
        "import { large } from 'mcp:fixture';\n\
         try { await large({ bytes: 6000000 }); console.log('UNEXPECTED'); }\n\
         catch (e: any) { console.log('KIND=' + e.kind); }\n",
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("KIND=call_failed"), "{}", out.stdout);
}
