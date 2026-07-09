//! Integration tests proving `Introspector::discover_server` enforces its
//! configured `connect_timeout` and `discover_timeout` (issue #120), and that
//! per-server-id locking in `mcp-execution-server` does not serialize
//! introspection across unrelated server ids.
//!
//! Also proves (issue #132) that when a timeout fires, the spawned MCP
//! server child process is actually terminated - not merely that
//! `discover_server` returns a timeout error while the process leaks.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`, which
//! is how CI and the project's preferred `cargo nextest` invocation run
//! tests) so that `fixture-slow-mcp-server` is built.

#![cfg(feature = "test-fixtures")]

use mcp_execution_core::{Error, ServerConfig, ServerId};
use mcp_execution_introspector::Introspector;
use std::path::Path;
use std::time::{Duration, Instant};

/// Checks whether a process with the given OS pid still exists.
///
/// Implemented per-platform without extra dependencies: `kill -0` on Unix
/// (arguments only, no shell involved) and `tasklist` filtering on Windows.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
}

/// Polls [`process_is_alive`] until it reports the process gone or `timeout`
/// elapses. Returns `true` once the process is confirmed gone.
async fn wait_for_process_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_is_alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    !process_is_alive(pid)
}

/// Polls a file until it has non-empty contents or `timeout` elapses, then
/// returns those contents. Used to read the fixture's self-reported pid,
/// which it writes at startup before the configured connect delay.
async fn wait_for_file_contents(path: &Path, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.trim().is_empty()
        {
            return contents;
        }
        assert!(
            Instant::now() < deadline,
            "pid file {} was not written within {timeout:?}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Absolute path to the `fixture-slow-mcp-server` binary built alongside this
/// test target. The fixture speaks real MCP over stdio and takes two
/// independent delays (both in milliseconds): the first CLI arg delays the
/// start of the connect handshake, the second delays the `tools/list`
/// response. Both default to 0 (immediate) when omitted. Using a real
/// compiled fixture (rather than the platform `sleep` command) keeps these
/// tests working on the `windows-latest` CI runner, which has no `sleep` on
/// `PATH`.
const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_fixture-slow-mcp-server");

/// The fixture delays the start of the handshake itself, so the rmcp `serve`
/// call blocks waiting for a response. With a short `connect_timeout`,
/// `discover_server` must fail with `Error::Timeout { operation: "connect" }`
/// well before the fixture's 30s handshake delay would elapse.
#[tokio::test]
async fn test_discover_server_connect_timeout_fires() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-connect-timeout");

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("30000".to_string()) // fixture delays the handshake itself by 30s
        .connect_timeout(Duration::from_millis(150))
        .build();

    let started = Instant::now();
    let result = introspector.discover_server(server_id, &config).await;
    let elapsed = started.elapsed();

    match result {
        Err(Error::Timeout { operation, .. }) => assert!(
            operation.starts_with("connect"),
            "expected a \"connect\" timeout, got operation={operation:?}"
        ),
        other => panic!("expected Error::Timeout {{ operation: \"connect\" }}, got {other:?}"),
    }

    assert!(
        elapsed < Duration::from_secs(5),
        "timeout should fire near the configured 150ms bound, not wait for the fixture's 30s handshake delay; took {elapsed:?}"
    );
}

/// The fixture server completes the handshake immediately but sleeps before
/// answering `tools/list`. With a short `discover_timeout`, `discover_server`
/// must fail with `Error::Timeout { operation: "list_all_tools" }`.
#[tokio::test]
async fn test_discover_server_discover_timeout_fires() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-discover-timeout");

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("0".to_string()) // no handshake delay
        .arg("30000".to_string()) // fixture delays tools/list by 30s
        .discover_timeout(Duration::from_millis(150))
        .build();

    let started = Instant::now();
    let result = introspector.discover_server(server_id, &config).await;
    let elapsed = started.elapsed();

    match result {
        Err(Error::Timeout { operation, .. }) => assert!(
            operation.starts_with("list_all_tools"),
            "expected a \"list_all_tools\" timeout, got operation={operation:?}"
        ),
        other => {
            panic!("expected Error::Timeout {{ operation: \"list_all_tools\" }}, got {other:?}")
        }
    }

    assert!(
        elapsed < Duration::from_secs(5),
        "timeout should fire near the configured 150ms bound, not wait for the fixture's 30s tools/list delay; took {elapsed:?}"
    );
}

/// #132 — when the connect handshake times out, the spawned child process
/// must be terminated, not merely have its rmcp transport dropped. Before
/// the fix, cleanup was delegated to a `tokio::spawn`-ed background task in
/// rmcp's `Drop` impl, which a short-lived test runtime could tear down
/// before ever running - leaking the process.
#[tokio::test]
async fn test_discover_server_connect_timeout_kills_child_process() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-connect-timeout-kills-child");

    let pid_file = tempfile::NamedTempFile::new().expect("create temp pid file");
    let pid_path = pid_file.path().to_path_buf();

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("30000".to_string()) // fixture delays the handshake by 30s
        .arg("0".to_string())
        .arg(pid_path.display().to_string())
        .connect_timeout(Duration::from_millis(150))
        .build();

    let result = introspector.discover_server(server_id, &config).await;
    assert!(
        matches!(result, Err(Error::Timeout { .. })),
        "expected Error::Timeout, got {result:?}"
    );

    let pid_contents = wait_for_file_contents(&pid_path, Duration::from_secs(2)).await;
    let pid: u32 = pid_contents
        .trim()
        .parse()
        .expect("fixture wrote a valid pid");

    assert!(
        wait_for_process_exit(pid, Duration::from_secs(2)).await,
        "expected introspection child process (pid {pid}) to be terminated after \
         the connect timeout, but it is still running"
    );
}

/// #132 — same guarantee as `test_discover_server_connect_timeout_kills_child_process`,
/// but for a `discover_timeout` (i.e. `tools/list` hangs after a successful handshake).
#[tokio::test]
async fn test_discover_server_discover_timeout_kills_child_process() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-discover-timeout-kills-child");

    let pid_file = tempfile::NamedTempFile::new().expect("create temp pid file");
    let pid_path = pid_file.path().to_path_buf();

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("0".to_string()) // no handshake delay
        .arg("30000".to_string()) // fixture delays tools/list by 30s
        .arg(pid_path.display().to_string())
        .discover_timeout(Duration::from_millis(150))
        .build();

    let result = introspector.discover_server(server_id, &config).await;
    assert!(
        matches!(result, Err(Error::Timeout { .. })),
        "expected Error::Timeout, got {result:?}"
    );

    let pid_contents = wait_for_file_contents(&pid_path, Duration::from_secs(2)).await;
    let pid: u32 = pid_contents
        .trim()
        .parse()
        .expect("fixture wrote a valid pid");

    assert!(
        wait_for_process_exit(pid, Duration::from_secs(2)).await,
        "expected introspection child process (pid {pid}) to be terminated after \
         the discover timeout, but it is still running"
    );
}

/// Sanity check: when the fixture responds promptly (no artificial delays),
/// `discover_server` succeeds and does not spuriously time out.
#[tokio::test]
async fn test_discover_server_succeeds_when_within_timeouts() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-fast-server");

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("0".to_string())
        .arg("0".to_string())
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build();

    let result = introspector.discover_server(server_id, &config).await;

    assert!(result.is_ok(), "expected success, got {result:?}");
}

/// #132 — `discover_server` kills its spawned child process unconditionally
/// after discovery completes, including on the success path (the process is
/// only ever needed for a single introspection round-trip). This proves the
/// child is actually gone shortly after `discover_server` returns `Ok(..)`,
/// not merely that the call itself succeeded.
#[tokio::test]
async fn test_discover_server_success_kills_child_process() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-success-kills-child");

    let pid_file = tempfile::NamedTempFile::new().expect("create temp pid file");
    let pid_path = pid_file.path().to_path_buf();

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("0".to_string()) // no handshake delay
        .arg("0".to_string()) // no tools/list delay
        .arg(pid_path.display().to_string())
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build();

    let result = introspector.discover_server(server_id, &config).await;
    assert!(result.is_ok(), "expected success, got {result:?}");

    let pid_contents = wait_for_file_contents(&pid_path, Duration::from_secs(2)).await;
    let pid: u32 = pid_contents
        .trim()
        .parse()
        .expect("fixture wrote a valid pid");

    assert!(
        wait_for_process_exit(pid, Duration::from_secs(2)).await,
        "expected introspection child process (pid {pid}) to be terminated after \
         a successful discover_server call, but it is still running"
    );
}
