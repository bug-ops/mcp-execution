//! The `mcp:helpers` module under `run`: `mapBounded` stops starting new items once one callback
//! rejects, so a failing item does not trigger the calls of the items after it.
//!
//! Unix only: the tests need `mcp.json`, which cannot be redirected on Windows (#604).

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use common::{Sandbox, require_node};

#[test]
fn map_bounded_stops_issuing_calls_after_a_callback_rejects() {
    if !require_node("map_bounded_stops_issuing_calls_after_a_callback_rejects") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let script = sb.script(
        "bounded.ts",
        "import { echo } from 'mcp:fixture';\n\
         import { mapBounded } from 'mcp:helpers';\n\
         try {\n\
           await mapBounded([0, 1, 2, 3, 4, 5, 6, 7, 8, 9], async (item: number) => {\n\
             if (item === 2) throw new Error('item 2 failed');\n\
             return echo({ item });\n\
           }, { concurrency: 1 });\n\
           console.log('RESULT=resolved');\n\
         } catch (error) {\n\
           console.log('RESULT=rejected');\n\
         }\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("RESULT=rejected"), "{}", out.stdout);
    let calls = sb
        .fixture_log()
        .iter()
        .filter(|l| *l == "call echo")
        .count();
    assert_eq!(calls, 2, "only the items before the failing one may call");
}
