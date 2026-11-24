//! Scenario harness of the context cost (#562, ADR-562, FR-046.13): the bytes that reach a
//! model's context when K tool calls are made one `call` per shell command (arm A) and when
//! one `run` script makes them (arm B), against the in-repo fixture server.
//!
//! "Reaching the context" is the command text the model writes plus everything the command
//! prints (stdout and stderr). Offline, no model: the script text is fixed, not model-authored,
//! so the figures prove only these scenarios, not the cost of writing a script, turn overhead or
//! prompt caching. Tokens are bytes/4, an estimate (ADR-562); bytes are always printed.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`) and Node.js 22.18+.
//! Run with `--no-capture` to see the table.

#![cfg(feature = "test-fixtures")]

mod common;

use std::fmt::Write as _;

use common::{Out, Sandbox, require_node};

/// Numbers of calls compared; always includes 1 and 2.
const CALL_COUNTS: [usize; 6] = [1, 2, 3, 5, 10, 20];

/// Divisor of the token estimate (ADR-562, option A).
const BYTES_PER_TOKEN_ESTIMATE: usize = 4;

/// Bytes reaching the context of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ContextBytes(usize);

impl ContextBytes {
    /// Command text plus both output streams of `out`.
    const fn of(command_text: &str, out: &Out) -> Self {
        Self(command_text.len() + out.stdout.len() + out.stderr.len())
    }

    const fn estimated_tokens(self) -> usize {
        self.0.div_ceil(BYTES_PER_TOKEN_ESTIMATE)
    }
}

/// Arm A: `count` calls, one shell command each.
fn arm_calls(sb: &Sandbox, count: usize) -> ContextBytes {
    let mut total = 0;
    for n in 0..count {
        let json = format!(r#"{{"n":{n}}}"#);
        let args = [
            "call",
            "--from-config",
            "fixture",
            "--",
            "echo",
            json.as_str(),
        ];
        let out = sb.cli(&args);
        assert_eq!(out.code, Some(0), "call {n} failed: {}", out.stderr);
        let command_text = format!("mcp-execution-cli {}", args.join(" "));
        total += ContextBytes::of(&command_text, &out).0;
    }
    ContextBytes(total)
}

/// Arm B: one `run -` whose script makes `count` calls and prints each result.
fn arm_script(sb: &Sandbox, count: usize) -> ContextBytes {
    let script = format!(
        "import {{ echo }} from 'mcp:fixture';\n\
         for (let n = 0; n < {count}; n++) console.log(JSON.stringify(await echo({{ n }})));\n"
    );
    let out = sb.cli_stdin(&["run", "-"], &script);
    assert_eq!(out.code, Some(0), "run failed: {}", out.stderr);
    assert_eq!(
        out.stdout.matches(r#"{"n":"#).count(),
        count,
        "every call result must be printed: {}",
        out.stdout
    );
    ContextBytes::of(&format!("mcp-execution-cli run -\n{script}"), &out)
}

/// The smallest compared K from which arm B stays below arm A, if any.
fn crossing(rows: &[(usize, ContextBytes, ContextBytes)]) -> Option<usize> {
    let from = rows
        .iter()
        .rposition(|&(_, a, b)| b >= a)
        .map_or(0, |i| i + 1);
    rows.get(from).map(|&(k, _, _)| k)
}

/// What the table does not prove (FR-046.16), printed under every table.
const NOT_PROVEN: &str = "\nNot proven by these figures: the script is fixed, not written by a model, so \
the cost of writing it is not counted; the server is the in-repo fixture only; \
tokens are an estimate (bytes/4), not a real count; turn overhead and prompt caching are not modelled.\n";

fn render(rows: &[(usize, ContextBytes, ContextBytes)], crossing: Option<usize>) -> String {
    let mut out = String::from(
        "Bytes reaching the context for K calls (command text + stdout + stderr).\n\
         Tokens are an estimate (bytes/4). A: one `call` per shell command; B: one `run` script.\n\n",
    );
    let _ = writeln!(
        out,
        "{:>4} {:>12} {:>14} {:>12} {:>14}",
        "K", "A bytes", "A tokens (est)", "B bytes", "B tokens (est)"
    );
    for &(k, a, b) in rows {
        let _ = writeln!(
            out,
            "{k:>4} {:>12} {:>14} {:>12} {:>14}",
            a.0,
            a.estimated_tokens(),
            b.0,
            b.estimated_tokens()
        );
    }
    let _ = match crossing {
        Some(k) => writeln!(out, "\nArm B is smaller from K = {k}."),
        None => writeln!(out, "\nArm B is never smaller within the compared K."),
    };
    out.push_str(NOT_PROVEN);
    out
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn script_arm_overtakes_one_call_per_command_within_the_compared_counts() {
    if !require_node("script_arm_overtakes_one_call_per_command_within_the_compared_counts") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();

    let rows: Vec<_> = CALL_COUNTS
        .iter()
        .map(|&k| (k, arm_calls(&sb, k), arm_script(&sb, k)))
        .collect();
    let crossing = crossing(&rows);
    let report = render(&rows, crossing);
    println!("{report}");

    assert!(
        rows.windows(2).all(|w| w[0].1 < w[1].1),
        "arm A grows with K"
    );
    let (_, a_first, b_first) = rows[0];
    assert!(
        a_first < b_first,
        "one call is cheaper as a plain call than as a script\n{report}"
    );
    let crossing = crossing.unwrap_or_else(|| panic!("the arms never cross\n{report}"));
    assert!(crossing > CALL_COUNTS[0], "{report}");
    assert!(
        rows.iter()
            .filter(|&&(k, ..)| k >= crossing)
            .all(|&(_, a, b)| b < a),
        "arm B must stay smaller after the crossing\n{report}"
    );
    assert!(!report.to_lowercase().contains("saving"));
    for stated in [
        "script is fixed, not written by a model",
        "cost of writing it is not counted",
        "in-repo fixture only",
        "estimate (bytes/4)",
    ] {
        assert!(report.contains(stated), "missing {stated:?}\n{report}");
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn script_arm_size_is_deterministic() {
    if !require_node("script_arm_size_is_deterministic") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();

    let first = (arm_calls(&sb, 3), arm_script(&sb, 3));
    let second = (arm_calls(&sb, 3), arm_script(&sb, 3));

    assert_eq!(first, second);
}
