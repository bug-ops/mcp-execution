//! Integration tests that compile and execute the generated `_runtime/mcp-bridge.ts` under
//! Node against fake MCP servers.

mod support;

use mcp_execution_codegen::progressive::ProgressiveGenerator;
use serde_json::json;
use std::collections::HashMap;
use support::{
    SINGLE_CALL_HARNESS_TS, compile_and_run_bridge_harness, create_test_server_info,
    respond_once_fake_server_js, run_bridge_harness, run_bridge_harness_with_env, test_config,
    write_test_script,
};

/// Behavioral regression guard for #201: a hostile `~/.claude/mcp.json` entry (a forbidden
/// `LD_PRELOAD` env var) must be rejected by the rendered `_runtime/mcp-bridge.ts` before it
/// ever spawns the configured subprocess. Unlike a string-grep over the rendered source (see
/// `test_generate_runtime_bridge_declares_forbidden_env_var_list` in
/// `progressive/generator.rs`, which can pass even against dead/unreachable validation code),
/// this actually compiles and executes the bridge under Node.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_forbidden_env_var_before_spawn() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    // Benign command/args (no shell metacharacters) so the metacharacter check doesn't fire
    // first and mask the env-var check under test.
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "LD_PRELOAD": "/tmp/evil.so" }
            }
        }
    });

    // `require_ts_toolchain` (called inside `run_bridge_harness`) already prints the skip
    // reason locally and hard-fails in CI, so a missing toolchain here just means "skip".
    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_forbidden_env_var_before_spawn",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the hostile LD_PRELOAD config before spawning:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("LD_PRELOAD"),
        "rejection reason should name the forbidden env var: {stdout}"
    );

    // Case-varied regression guard for #428: the bridge's `validateEnvName` must reject a
    // differently-cased spelling of a forbidden name too, mirroring the Rust-side
    // `validate_env_name` fix — Windows treats environment variable names as
    // case-insensitive, so `Ld_Preload` is just as real a `LD_PRELOAD` override as the
    // canonical spelling once the subprocess is spawned.
    let mcp_json_case_varied = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "Ld_Preload": "/tmp/evil.so" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_forbidden_env_var_before_spawn_case_varied",
        &bridge.content,
        &mcp_json_case_varied,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the case-varied Ld_Preload config before spawning:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the case-varied config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("Ld_Preload"),
        "rejection reason should name the forbidden env var as passed: {stdout}"
    );

    // Case-varied regression guard for the `DYLD_` *prefix* branch specifically (the exact-name
    // and prefix checks are separate code paths in `validateEnvName`, so the exact-name case
    // above does not exercise this one).
    let mcp_json_dyld_prefix_case_varied = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "dyld_insert_libraries": "/tmp/evil.so" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_forbidden_env_var_before_spawn_dyld_prefix_case_varied",
        &bridge.content,
        &mcp_json_dyld_prefix_case_varied,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the case-varied dyld_insert_libraries config before spawning:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the case-varied prefix config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("dyld_insert_libraries"),
        "rejection reason should name the forbidden env var as passed: {stdout}"
    );
}

/// #467 — a hostile `~/.claude/mcp.json` entry can carry an env var name outside the
/// `[A-Za-z_][A-Za-z0-9_]*` identifier charset (e.g. containing a hyphen or space) without
/// matching any entry on `FORBIDDEN_ENV_NAMES`; before this fix, `validateEnvName` had no
/// charset check at all, so such a name passed through unrejected even though
/// `mcp_execution_core::validate_env_name` (added by #438) already rejects it.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_env_name_outside_charset() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "MY-VAR": "value" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_env_name_outside_charset",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the charset-violating env name cleanly:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("valid identifier"),
        "rejection reason should mention the charset requirement: {stdout}"
    );
}

/// #471 — a hostile `~/.claude/mcp.json` entry can carry more positional arguments than
/// `mcp_execution_core::MAX_ARG_COUNT` allows (denial-of-service protection, CWE-400); the
/// rendered bridge must reject this before spawning, mirroring
/// `validate_stdio_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_too_many_arguments() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let too_many_args: Vec<String> = (0..=mcp_execution_core::MAX_ARG_COUNT)
        .map(|i| i.to_string())
        .collect();
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": too_many_args
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_too_many_arguments",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject too many arguments cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("too many arguments"),
        "rejection reason should mention the argument count bound: {stdout}"
    );
}

/// #471 — a single argument longer than `mcp_execution_core::MAX_ARG_LEN` bytes must be
/// rejected before spawning, mirroring `validate_stdio_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_argument_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_arg = "a".repeat(mcp_execution_core::MAX_ARG_LEN + 1);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": [long_arg]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_argument_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized argument cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("argument 0") && stdout.contains("too long"),
        "rejection reason should name the argument and the length bound: {stdout}"
    );
}

/// #471 — more environment variables than `mcp_execution_core::MAX_ENV_COUNT` must be
/// rejected before spawning, mirroring `validate_stdio_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_too_many_env_vars() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let too_many_env: HashMap<String, String> = (0..=mcp_execution_core::MAX_ENV_COUNT)
        .map(|i| (format!("VAR_{i}"), "value".to_string()))
        .collect();
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": too_many_env
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_too_many_env_vars",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject too many env vars cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("too many environment variables"),
        "rejection reason should mention the env count bound: {stdout}"
    );
}

/// #471 — an environment variable value longer than `mcp_execution_core::MAX_ENV_VALUE_LEN`
/// bytes must be rejected before spawning, mirroring `validate_stdio_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_env_value_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_value = "a".repeat(mcp_execution_core::MAX_ENV_VALUE_LEN + 1);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "MY_VAR": long_value }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_env_value_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized env value cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("environment variable 'MY_VAR' value too long"),
        "rejection reason should name the env var and the length bound: {stdout}"
    );
}

/// Behavioral regression guard for #201's critic follow-up (S3): `{"transport":"http",...}`
/// is a valid `mcp.json` entry since #200 (all `ServerConfig` fields are `#[serde(default)]`
/// on the Rust side). Before this fix, the bridge's validator called `.trim()` on the
/// `undefined` `command`/`args` an http-transport entry has, throwing an opaque
/// `TypeError: Cannot read properties of undefined (reading 'trim')`. This runtime bridge
/// only ever spawns a subprocess (stdio transport); a non-stdio config must be rejected with
/// a clear, intentional error instead of that crash.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_with_clear_error_not_crash() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp"
            }
        }
    });

    // `require_ts_toolchain` (called inside `run_bridge_harness`) already prints the skip
    // reason locally and hard-fails in CI, so a missing toolchain here just means "skip".
    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_with_clear_error_not_crash",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the http-transport config cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties of undefined"),
        "must fail with a clear, intentional error, not the pre-fix opaque TypeError: {stdout}"
    );
    assert!(
        stdout.contains("http") || stdout.contains("transport"),
        "rejection reason should mention the unsupported transport: {stdout}"
    );
}

/// #221 item 2 — the rendered bridge must validate an http/sse config's URL scheme to the
/// same depth as `mcp_execution_core::validate_server_config` before rejecting the transport
/// as unsupported, so a bad scheme is reported precisely rather than masked by the generic
/// "unsupported transport" message.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_bad_url_scheme() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    for url in ["file:///etc/passwd", "ftp://host/path"] {
        let mcp_json = json!({
            "mcpServers": {
                "github": {
                    "transport": "http",
                    "url": url
                }
            }
        });

        let Some((success, stdout, stderr)) = run_bridge_harness(
            "test_runtime_bridge_rejects_http_transport_bad_url_scheme",
            &bridge.content,
            &mcp_json,
            "github",
        ) else {
            return;
        };

        assert!(
            success,
            "bridge did not reject url {url}:\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            stdout.contains("http:// or https://"),
            "rejection reason should name the required scheme for url {url}: {stdout}"
        );
    }
}

/// #221 item 2 — a hand-edited `mcp.json` with `"transport": "http"` and no `url` key is
/// valid JSON (every `ServerConfig` field is optional on the Rust side); the bridge must
/// reject it with a specific "url is required" message, not an opaque `TypeError`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_missing_url() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http"
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_missing_url",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject missing url cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties of undefined"),
        "must fail with a clear, intentional error, not an opaque TypeError: {stdout}"
    );
    assert!(
        stdout.contains("url is required"),
        "rejection reason should mention the missing url: {stdout}"
    );
}

/// #221 item 2 — header name/value safety for http/sse transports, mirroring
/// `mcp_execution_core::command`'s RFC 7230 `tchar` and control-character checks.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_unsafe_headers() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    // Space is not a control character but is still outside RFC 7230's `token` charset.
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "X Bad Header": "value" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_unsafe_headers",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the unsafe header name cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("header name"),
        "rejection reason should mention the header name: {stdout}"
    );
}

/// #221 item 2 — a header value containing a CR/LF must be rejected, and — like the Rust
/// source of truth — the value itself (which routinely carries secrets such as bearer
/// tokens) must never appear in the thrown error.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_control_char_in_header_value() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": "Bearer sekrit\r\nX-Injected: evil" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_control_char_in_header_value",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the unsafe header value cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Authorization"),
        "rejection reason should name the header: {stdout}"
    );
    assert!(
        !stdout.contains("sekrit") && !stdout.contains("X-Injected"),
        "the header VALUE must never appear in the error message: {stdout}"
    );
}

/// #221 item 2 — two header names differing only in case (e.g. `Authorization` and
/// `authorization`) must be rejected, mirroring the Rust source of truth's
/// case-insensitive-collision check.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_duplicate_case_insensitive_headers() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": "Bearer one", "authorization": "Bearer two" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_duplicate_case_insensitive_headers",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the duplicate header cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("duplicate header"),
        "rejection reason should mention the duplicate header: {stdout}"
    );
}

/// #471 — a `url` longer than `mcp_execution_core::MAX_URL_LEN` bytes must be rejected,
/// mirroring `validate_network_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_url_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let padding = "a".repeat(mcp_execution_core::MAX_URL_LEN);
    let long_url = format!("https://example.com/{padding}");
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": long_url
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_url_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized url cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("url for server") && stdout.contains("too long"),
        "rejection reason should name the url and the length bound: {stdout}"
    );
}

/// #471 — more headers than `mcp_execution_core::MAX_HEADER_COUNT` must be rejected, mirroring
/// `validate_network_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_too_many_headers() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let too_many_headers: HashMap<String, String> = (0..=mcp_execution_core::MAX_HEADER_COUNT)
        .map(|i| (format!("X-Header-{i}"), "value".to_string()))
        .collect();
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": too_many_headers
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_too_many_headers",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject too many headers cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("too many headers"),
        "rejection reason should mention the header count bound: {stdout}"
    );
}

/// #471 — a header value longer than `mcp_execution_core::MAX_HEADER_VALUE_LEN` bytes must be
/// rejected, mirroring `validate_network_size_bounds`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_header_value_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_value = "a".repeat(mcp_execution_core::MAX_HEADER_VALUE_LEN + 1);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": long_value }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_header_value_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized header value cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("value for header 'Authorization'") && stdout.contains("too long"),
        "rejection reason should name the header and the length bound: {stdout}"
    );
}

/// #471/#467 critique S1 — a non-string environment variable value (here, a JSON array) must
/// be rejected outright rather than silently skipped by the length check: `spawn`'s `env`
/// option coerces a non-string value via `String()`, so an unrejected array could carry
/// arbitrarily more data than `MAX_ENV_VALUE_LEN` while never being measured as a string.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_non_string_env_value() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "FOO": ["evil", "evil", "evil"] }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_non_string_env_value",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject a non-string env value cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("environment variable 'FOO' value") && stdout.contains("must be a string"),
        "rejection reason should say the env value must be a string: {stdout}"
    );
}

/// #471/#467 critique S1 — a non-string header value (here, a number) must be rejected
/// outright, mirroring the env-value fix above for the http/sse transport's own size-bound
/// check.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_non_string_header_value() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": 12345 }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_non_string_header_value",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject a non-string header value cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("value for header 'Authorization'") && stdout.contains("must be a string"),
        "rejection reason should say the header value must be a string: {stdout}"
    );
}

/// #471/#467 critique S5 — a string `env` (not an object) must be rejected outright rather
/// than treated as zero entries. An earlier revision of `recordEntries` returned `[]` for a
/// non-object `env`/`headers` value, which silently disabled `MAX_ENV_COUNT`,
/// `MAX_ENV_VALUE_LEN`, and `validateEnvName` entirely — verified under node that
/// `{ ...process.env, ...'x'.repeat(2000) }` really produces 2000 numeric-keyed environment
/// variables passed to `spawn`, none of them length- or count-checked.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_string_env() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": "a".repeat(2000)
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_string_env",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject a string env cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("env for server") && stdout.contains("must be an object"),
        "rejection reason should say env must be an object: {stdout}"
    );
}

/// #471/#467 critique S5 — an array `env` must be rejected outright for the same reason as a
/// string `env` (see `test_runtime_bridge_rejects_string_env`): each array element keeps its
/// full length once spread into `spawn`'s `env` option, so `MAX_ENV_VALUE_LEN` would fail even
/// harder than for a string `env` if this were merely skipped rather than rejected.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_array_env() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": ["a".repeat(2000), "b".repeat(2000)]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_array_env",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an array env cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("env for server") && stdout.contains("must be an object"),
        "rejection reason should say env must be an object: {stdout}"
    );
}

/// #471 critique M2 — `byteLength` (UTF-8 bytes), not `.length` (UTF-16 code units), must
/// drive the length check. `'é'` is one UTF-16 code unit but two UTF-8 bytes: at exactly
/// `MAX_ARG_LEN` repetitions, `.length` sits at the cap (would NOT reject under a `.length`
/// regression) while the UTF-8 byte length is double the cap (MUST reject). This is the
/// tightest-margin proof that the port uses byte length, not code-unit length.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_argument_too_long_multibyte_utf8() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_arg = "é".repeat(mcp_execution_core::MAX_ARG_LEN);
    assert_eq!(long_arg.chars().count(), mcp_execution_core::MAX_ARG_LEN);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": [long_arg]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_argument_too_long_multibyte_utf8",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized multi-byte argument cleanly:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("argument 0") && stdout.contains("too long"),
        "rejection reason should name the argument and the length bound — a `.length` \
         regression would accept this config instead: {stdout}"
    );
}

/// #471 critique M3 — the `command` field itself, not just `args`/`env`, must be
/// length-bounded, mirroring `validate_stdio_size_bounds`'s `command.len() > MAX_ARG_LEN` check.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_command_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_command = "a".repeat(mcp_execution_core::MAX_ARG_LEN + 1);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": long_command
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_command_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized command cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("command for server") && stdout.contains("too long"),
        "rejection reason should name the command and the length bound: {stdout}"
    );
}

/// #471 critique M3 — an environment variable *name* longer than `MAX_ARG_LEN` must be
/// rejected, mirroring `validate_stdio_size_bounds`'s `env_name.len() > MAX_ARG_LEN` check.
/// Uses an all-uppercase-letter name so it never also trips the #467 charset check, isolating
/// this specific bound.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_env_name_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_name = "A".repeat(mcp_execution_core::MAX_ARG_LEN + 1);
    let env: HashMap<String, String> = HashMap::from([(long_name, "value".to_string())]);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": env
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_env_name_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized env name cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("environment variable name too long"),
        "rejection reason should mention the env name length bound: {stdout}"
    );
}

/// #471 critique M3 — a header *name* longer than `MAX_ARG_LEN` must be rejected, mirroring
/// `validate_network_size_bounds`'s `name.len() > MAX_ARG_LEN` check.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_header_name_too_long() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let long_name = "X".repeat(mcp_execution_core::MAX_ARG_LEN + 1);
    let headers: HashMap<String, String> = HashMap::from([(long_name, "value".to_string())]);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": headers
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_header_name_too_long",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject an oversized header name cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("header name for server") && stdout.contains("too long"),
        "rejection reason should mention the header name length bound: {stdout}"
    );
}

/// #471 critique M1 — an at-cap element count/length must be *accepted* by the size-bound
/// checks (Rust's comparisons are `>`, not `>=`), the mirror-image of the over-cap tests
/// above. Spawns a deliberately nonexistent command so the outcome is deterministic
/// regardless of argument content/count: the OS never parses argv for a program that isn't
/// found, so this only proves the size checks let the config through, not anything about
/// `node`'s own CLI parsing.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_argument_count_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let args_at_cap: Vec<String> = (0..mcp_execution_core::MAX_ARG_COUNT)
        .map(|i| i.to_string())
        .collect();
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "mcp-execution-test-definitely-nonexistent-command-xyz",
                "args": args_at_cap
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_argument_count_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "harness did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected rejection (nonexistent command), just not from the size check: \
         stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("too many arguments"),
        "an at-cap argument count must be accepted by the size check: {stdout}"
    );
}

/// #471 critique M1 — mirror-image of `test_runtime_bridge_rejects_argument_too_long`. See
/// `test_runtime_bridge_accepts_argument_count_at_cap` for why a nonexistent command is used.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_argument_length_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let arg_at_cap = "a".repeat(mcp_execution_core::MAX_ARG_LEN);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "mcp-execution-test-definitely-nonexistent-command-xyz",
                "args": [arg_at_cap]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_argument_length_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "harness did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected rejection (nonexistent command), just not from the size check: \
         stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("too long"),
        "an at-cap argument length must be accepted by the size check: {stdout}"
    );
}

/// #471 critique M1 — mirror-image of `test_runtime_bridge_rejects_too_many_env_vars`. See
/// `test_runtime_bridge_accepts_argument_count_at_cap` for why a nonexistent command is used.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_env_count_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let env_at_cap: HashMap<String, String> = (0..mcp_execution_core::MAX_ENV_COUNT)
        .map(|i| (format!("VAR_{i}"), "value".to_string()))
        .collect();
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "mcp-execution-test-definitely-nonexistent-command-xyz",
                "args": [],
                "env": env_at_cap
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_env_count_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "harness did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected rejection (nonexistent command), just not from the size check: \
         stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("too many environment variables"),
        "an at-cap env var count must be accepted by the size check: {stdout}"
    );
}

/// #471 critique M1 — mirror-image of `test_runtime_bridge_rejects_env_value_too_long`. See
/// `test_runtime_bridge_accepts_argument_count_at_cap` for why a nonexistent command is used.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_env_value_length_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let value_at_cap = "a".repeat(mcp_execution_core::MAX_ENV_VALUE_LEN);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "mcp-execution-test-definitely-nonexistent-command-xyz",
                "args": [],
                "env": { "MY_VAR": value_at_cap }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_env_value_length_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "harness did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected rejection (nonexistent command), just not from the size check: \
         stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("value too long"),
        "an at-cap env value length must be accepted by the size check: {stdout}"
    );
}

/// #471 critique M1 — mirror-image of `test_runtime_bridge_rejects_http_transport_url_too_long`.
/// Uses the same "validated then rejected as unsupported transport" pattern as the other
/// http-transport tests: an at-cap url passes size validation, so the only rejection reason
/// left is the (deterministic, no process spawn) "unsupported transport" message.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_url_length_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let prefix = "https://e.co/";
    let padding = "a".repeat(mcp_execution_core::MAX_URL_LEN - prefix.len());
    let url_at_cap = format!("{prefix}{padding}");
    assert_eq!(url_at_cap.len(), mcp_execution_core::MAX_URL_LEN);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": url_at_cap
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_url_length_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:") && stdout.contains("only supports stdio"),
        "expected rejection for unsupported transport, not a size-check failure: {stdout}"
    );
    assert!(
        !stdout.contains("too long"),
        "an at-cap url length must be accepted by the size check: {stdout}"
    );
}

/// #471 critique M1 — mirror-image of `test_runtime_bridge_rejects_http_transport_too_many_headers`.
/// See `test_runtime_bridge_accepts_url_length_at_cap` for the "unsupported transport" pattern
/// this relies on.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_header_count_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let headers_at_cap: HashMap<String, String> = (0..mcp_execution_core::MAX_HEADER_COUNT)
        .map(|i| (format!("X-Header-{i}"), "value".to_string()))
        .collect();
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": headers_at_cap
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_header_count_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:") && stdout.contains("only supports stdio"),
        "expected rejection for unsupported transport, not a size-check failure: {stdout}"
    );
    assert!(
        !stdout.contains("too many headers"),
        "an at-cap header count must be accepted by the size check: {stdout}"
    );
}

/// #471 critique M1 — mirror-image of
/// `test_runtime_bridge_rejects_http_transport_header_value_too_long`. See
/// `test_runtime_bridge_accepts_url_length_at_cap` for the "unsupported transport" pattern
/// this relies on.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_accepts_header_value_length_at_cap() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let value_at_cap = "a".repeat(mcp_execution_core::MAX_HEADER_VALUE_LEN);
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": value_at_cap }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_accepts_header_value_length_at_cap",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not settle cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:") && stdout.contains("only supports stdio"),
        "expected rejection for unsupported transport, not a size-check failure: {stdout}"
    );
    assert!(
        !stdout.contains("too long"),
        "an at-cap header value length must be accepted by the size check: {stdout}"
    );
}

/// #221 item 4 — a subprocess that exits before ever writing a JSON-RPC response must be
/// rejected with a clear error instead of hanging forever. Reproduces the issue's own
/// repro steps almost verbatim (`node -e "process.exit(1)"`, via a script file — see
/// `write_test_script`).
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_when_child_exits_before_responding() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script("process.exit(1);\n");
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": [script_path]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_when_child_exits_before_responding",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the dead server cleanly (may have hung until the harness \
         watchdog fired):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the call: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("exited before responding"),
        "rejection reason should explain the process died before replying: {stdout}"
    );
}

/// #221 item 4 — a subprocess that spawns successfully but never replies at all must still
/// fail with a clear timeout error rather than hang forever. Uses
/// `MCPBRIDGE_REQUEST_TIMEOUT_MS` (well under the harness's 5s watchdog) so this test doesn't
/// need to wait out the bridge's real 30s default.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_times_out_when_server_never_replies() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    // Stays alive indefinitely without ever writing to stdout.
    let script_path = write_test_script("setInterval(function keepAlive() {}, 1000);\n");
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": [script_path]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness_with_env(
        "test_runtime_bridge_times_out_when_server_never_replies",
        &bridge.content,
        &mcp_json,
        "github",
        &[("MCPBRIDGE_REQUEST_TIMEOUT_MS", "200")],
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not time out cleanly (may have hung until the harness watchdog \
         fired):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the call: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("Timed out"),
        "rejection reason should explain the request timed out: {stdout}"
    );
}

/// Regression guard for #232: two concurrent `callMCPTool` calls on the same connection must
/// each resolve with the response matching THEIR OWN JSON-RPC request id, even when the
/// server replies out of order (responds to the second request before the first). Before the
/// fix, each call's response was consumed by whichever per-call listener happened to fire
/// first — not matched by id — so an out-of-order reply silently handed one caller the other's
/// result.
///
/// The fake MCP server below deliberately buffers both `tools/call` requests and replies to
/// the second-received one first, echoing back each call's own argument so a mismatch is
/// directly observable in the result value rather than merely "no crash occurred".
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One end-to-end scenario (generate -> embed fake out-of-order MCP server -> \
              compile -> run -> assert) whose fake-server script and assertions must stay in \
              one body to remain readable."
)]
fn test_runtime_bridge_dispatches_concurrent_out_of_order_responses_by_request_id() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let fake_server_js = r"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin, terminal: false });
const pendingToolCalls = [];

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (trimmed.length === 0) return;
  const message = JSON.parse(trimmed);

  if (message.method === 'initialize') {
    const response = {
      jsonrpc: '2.0',
      id: message.id,
      result: {
        protocolVersion: '2024-11-05',
        capabilities: {},
        serverInfo: { name: 'fake', version: '0.0.0' }
      }
    };
    process.stdout.write(JSON.stringify(response) + '\n');
    return;
  }

  if (message.method === 'tools/call') {
    pendingToolCalls.push(message);
    if (pendingToolCalls.length < 2) return;

    // Deliberately reply in the REVERSE of arrival order: the second request received gets
    // its response written first. This is the out-of-order scenario from issue #232 — a
    // correct dispatcher must still resolve each caller with the response matching ITS OWN
    // request id, not whichever response happens to arrive first.
    const [receivedFirst, receivedSecond] = pendingToolCalls;
    for (const call of [receivedSecond, receivedFirst]) {
      const response = {
        jsonrpc: '2.0',
        id: call.id,
        result: {
          content: [{ type: 'text', text: `${call.params.arguments.value}-response` }]
        }
      };
      process.stdout.write(JSON.stringify(response) + '\n');
    }
  }
});
";
    let script_path = write_test_script(fake_server_js);

    let mcp_json = json!({
        "mcpServers": {
            "fake": {
                "command": "node",
                "args": [script_path]
            }
        }
    });

    let harness_ts = r"
import { callMCPTool } from './mcp-bridge.js';

const watchdog = setTimeout(() => {
  console.log('TIMEOUT: did not settle both calls');
  process.exit(2);
}, 5000);

const first = callMCPTool('fake', 'echo', { value: 'first' });
const second = callMCPTool('fake', 'echo', { value: 'second' });

Promise.all([first, second]).then(
  ([firstResult, secondResult]) => {
    clearTimeout(watchdog);
    console.log('FIRST:', JSON.stringify(firstResult));
    console.log('SECOND:', JSON.stringify(secondResult));
    if (firstResult === 'first-response' && secondResult === 'second-response') {
      console.log('MATCH');
      process.exit(0);
    } else {
      console.log('MISMATCH');
      process.exit(1);
    }
  },
  (err: unknown) => {
    clearTimeout(watchdog);
    console.log('REJECTED:', String(err));
    process.exit(3);
  }
);
";

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_dispatches_concurrent_out_of_order_responses_by_request_id",
        &bridge.content,
        &mcp_json,
        harness_ts,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not dispatch out-of-order concurrent responses correctly:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("MATCH"),
        "each caller must resolve with the response matching its own request id, not the \
         other caller's response: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #255: a tool-error response (`isError: true`) with an empty `content`
/// array must surface as a clear rejection, not crash with an unguarded
/// `Cannot read properties of undefined (reading 'text')` `TypeError`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_surfaces_tool_error_with_empty_content() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "isError": true, "content": [] }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_surfaces_tool_error_with_empty_content",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:") && stdout.contains("Tool returned error"),
        "an empty content array on an isError response must reject with a clear \
         'Tool returned error' message, not crash on an unguarded content[0] dereference: \
         stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties"),
        "must not crash with an unguarded property-access TypeError: stdout: {stdout}"
    );
}

/// Regression guard for #255: a successful response carrying only `structuredContent` (no
/// `content`, per spec 2025-06-18+) must resolve with that `structuredContent` value as a
/// distinct, well-typed case, not crash or silently return `undefined`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_returns_structured_content_when_content_empty() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [], "structuredContent": { "answer": 42 } }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_returns_structured_content_when_content_empty",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(r#"RESOLVED: {"answer":42}"#),
        "a structuredContent-only response must resolve with structuredContent, not crash or \
         resolve with something else: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #255 and #482: a successful response with an empty `content` array and
/// no `structuredContent` at all is a valid empty success that resolves to `undefined`, not a
/// rejection and not a crash on an unguarded `content[0]` dereference.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_resolves_empty_content_without_structured_content_to_undefined() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(r#"{ "content": [] }"#));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_resolves_empty_content_without_structured_content_to_undefined",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("RESOLVED: undefined"),
        "an empty content array with no structuredContent must resolve to undefined, not \
         reject: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties"),
        "must not crash with an unguarded property-access TypeError: stdout: {stdout}"
    );
}

/// Regression guard for #262: a successful response whose `content` array contains a `null`
/// first element must surface as a clear rejection, not crash on an unguarded
/// `firstContent.type` dereference.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_null_first_content_element() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(r#"{ "content": [null] }"#));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_rejects_null_first_content_element",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "a null first content element must reject with a clear error, not crash or silently \
         resolve: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties"),
        "must not crash with an unguarded property-access TypeError: stdout: {stdout}"
    );
}

/// Regression guard for #262: a successful response whose `content` array contains a `null`
/// first element but also carries a populated `structuredContent` must resolve with the
/// `structuredContent` value instead of discarding it and throwing — a malformed `content[0]`
/// should not shadow otherwise-usable structured data.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_falls_back_to_structured_content_on_null_first_content_element() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [null], "structuredContent": { "answer": 42 } }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_falls_back_to_structured_content_on_null_first_content_element",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(r#"RESOLVED: {"answer":42}"#),
        "a null first content element with populated structuredContent must resolve with \
         structuredContent, not discard it and reject: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #262: a successful response carrying a literal `structuredContent: null`
/// alongside an empty `content` array must be treated the same as an absent `structuredContent`
/// (i.e. an empty success resolving to `undefined`), not returned as if `null` were real data.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_treats_null_structured_content_as_absent() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [], "structuredContent": null }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_treats_null_structured_content_as_absent",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("RESOLVED: undefined"),
        "a literal structuredContent: null must be treated as absent and resolve to undefined, \
         not resolve with null: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #262: an `isError: true` response with an empty `content` array but a
/// populated `structuredContent` must surface that structured detail in the error's
/// `serverMessage` (never in `message`) instead of discarding it.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_surfaces_structured_content_on_tool_error() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "isError": true, "content": [], "structuredContent": { "reason": "boom" } }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_surfaces_structured_content_on_tool_error",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:")
            && stdout.contains("Tool returned error")
            && stdout.contains(r#"SERVER_MESSAGE: {"reason":"boom"}"#),
        "an isError response with populated structuredContent must surface it in \
         serverMessage: stdout: {stdout}, stderr: {stderr}"
    );
    let rejection = stdout
        .lines()
        .find(|l| l.starts_with("REJECTED:"))
        .unwrap_or_default();
    assert!(
        !rejection
            .split(" SERVER_MESSAGE:")
            .next()
            .unwrap_or_default()
            .contains("boom"),
        "server text must stay out of the error message: {rejection}"
    );
}

fn rendered_bridge() -> String {
    let code = ProgressiveGenerator::new()
        .expect("Failed to create generator")
        .generate(&create_test_server_info(), &test_config())
        .expect("Failed to generate code");
    code.files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found")
        .content
        .clone()
}

fn fake_mcp_json(script_path: &str) -> serde_json::Value {
    json!({ "mcpServers": { "fake": { "command": "node", "args": [script_path] } } })
}

/// Fake server that answers `method` with the JSON-RPC `error` object `error_js` and everything
/// else normally.
fn failing_fake_server_js(method: &str, error_js: &str) -> String {
    format!(
        r"
const readline = require('readline');
const rl = readline.createInterface({{ input: process.stdin, terminal: false }});
rl.on('line', (line) => {{
  if (line.trim().length === 0) return;
  const message = JSON.parse(line);
  if (message.method === {method:?}) {{
    process.stdout.write(JSON.stringify({{ jsonrpc: '2.0', id: message.id, error: {error_js} }}) + '\n');
  }} else if (message.method === 'initialize') {{
    process.stdout.write(JSON.stringify({{ jsonrpc: '2.0', id: message.id, result: {{ protocolVersion: '2024-11-05', capabilities: {{}}, serverInfo: {{ name: 'fake', version: '0.0.0' }} }} }}) + '\n');
  }}
}});
"
    )
}

/// Runs one successful call with `MCPBRIDGE_OBSERVE_SHAPES` pointing at a file prepared by
/// `prepare`, returning that file's content afterwards (empty when it does not exist).
#[cfg(unix)]
fn run_with_recorder_target(test_name: &str, prepare: impl FnOnce(&std::path::Path)) -> String {
    let dir = tempfile::tempdir().expect("temp dir");
    let target = dir.path().join("observed.jsonl");
    prepare(&target);
    let script = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [{ "type": "text", "text": "{\"a\":1}" }] }"#,
    ));
    let target_arg = target.to_string_lossy().into_owned();
    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        test_name,
        &rendered_bridge(),
        &fake_mcp_json(&script),
        SINGLE_CALL_HARNESS_TS,
        &[("MCPBRIDGE_OBSERVE_SHAPES", &target_arg)],
    ) else {
        return "SKIPPED".to_string();
    };
    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains(r#"RESOLVED: {"a":1}"#),
        "the call itself must succeed whatever the recorder does: {stdout}"
    );
    if target.is_file() {
        std::fs::read_to_string(&target).unwrap_or_default()
    } else {
        String::new()
    }
}

#[cfg(unix)]
#[test]
fn test_runtime_bridge_recorder_skips_a_preexisting_group_readable_file() {
    use std::os::unix::fs::PermissionsExt as _;

    let recorded = run_with_recorder_target(
        "test_runtime_bridge_recorder_skips_a_preexisting_group_readable_file",
        |target| {
            std::fs::write(target, "seed\n").unwrap();
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o644)).unwrap();
        },
    );

    if recorded != "SKIPPED" {
        assert_eq!(recorded, "seed\n", "a 0644 file must not be appended to");
    }
}

#[cfg(unix)]
#[test]
fn test_runtime_bridge_recorder_does_not_follow_a_symlink() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().expect("temp dir");
    let victim = dir.path().join("victim.txt");
    std::fs::write(&victim, "victim\n").unwrap();
    // A private, user-owned regular file passes the ownership and mode checks, so only the
    // no-follow open can stop the append.
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o600)).unwrap();
    let victim_for_link = victim.clone();

    let _ = run_with_recorder_target(
        "test_runtime_bridge_recorder_does_not_follow_a_symlink",
        move |target| std::os::unix::fs::symlink(&victim_for_link, target).unwrap(),
    );

    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "victim\n");
}

#[test]
fn test_runtime_bridge_non_string_server_message_is_dropped_not_turned_into_call_failed() {
    let name =
        "test_runtime_bridge_non_string_server_message_is_dropped_not_turned_into_call_failed";
    let script = write_test_script(&failing_fake_server_js(
        "tools/call",
        "{ code: -32000, message: { nested: true } }",
    ));
    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        name,
        &rendered_bridge(),
        &fake_mcp_json(&script),
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    assert!(
        stdout.contains("REJECTED: McpToolError: Tool execution failed"),
        "{stdout}"
    );
    assert!(!stdout.contains("SERVER_MESSAGE"), "{stdout}");
}

#[test]
fn test_runtime_bridge_init_error_keeps_the_truncation_flag() {
    let name = "test_runtime_bridge_init_error_keeps_the_truncation_flag";
    let script = write_test_script(&failing_fake_server_js(
        "initialize",
        "{ code: -32000, message: 'a'.repeat(5000) }",
    ));
    let harness = r"
import { callMCPTool, McpToolError } from './mcp-bridge.js';
callMCPTool('fake', 'noop', {}).then(
  () => { console.log('UNEXPECTED_SUCCESS'); process.exit(0); },
  (e: unknown) => {
    const err = e as McpToolError;
    console.log(JSON.stringify({
      kind: err.kind,
      truncated: err.serverMessageTruncated,
      length: err.serverMessage?.length,
      message: err.message,
    }));
    process.exit(0);
  }
);
";
    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        name,
        &rendered_bridge(),
        &fake_mcp_json(&script),
        harness,
        &[],
    ) else {
        return;
    };

    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    let seen: serde_json::Value =
        serde_json::from_str(stdout.lines().last().unwrap_or_default()).expect("harness JSON");
    assert_eq!(seen["kind"], "rpc_error", "{seen}");
    assert_eq!(seen["truncated"], true, "{seen}");
    assert_eq!(seen["length"], 500, "{seen}");
    assert!(
        !seen["message"].as_str().unwrap().contains("aaaa"),
        "{seen}"
    );
}

#[cfg(unix)]
#[test]
fn test_runtime_bridge_recorder_does_not_block_on_a_fifo() {
    let recorded = run_with_recorder_target(
        "test_runtime_bridge_recorder_does_not_block_on_a_fifo",
        |target| {
            let made = std::process::Command::new("mkfifo")
                .arg(target)
                .status()
                .is_ok_and(|s| s.success());
            assert!(made, "mkfifo is required on unix test hosts");
        },
    );

    assert!(
        recorded.is_empty() || recorded == "SKIPPED",
        "nothing is written into a FIFO: {recorded:?}"
    );
}
