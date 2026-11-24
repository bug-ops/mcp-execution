//! Shared harness of the script-runtime acceptance tests: an isolated home with a fixture
//! `mcp.json`, a watchdog around every spawned process, and locators for runner state.
//!
//! Included only by test files gated on the `test-fixtures` feature, which builds
//! `fixture-tools-mcp-server`.

#![allow(
    dead_code,
    reason = "shared by several test crates; each uses a subset"
)]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test, bench and example code may panic on failure"
)]

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The CLI under test.
pub const CLI_BIN: &str = env!("CARGO_BIN_EXE_mcp-execution-cli");

/// The fixture MCP server with the fixed tool set.
pub const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_fixture-tools-mcp-server");

/// Server id the sandbox registers for the stdio fixture.
pub const SERVER: &str = "fixture";

/// Hard stop for any spawned process; a hang fails the test instead of stalling CI.
pub const WATCHDOG: Duration = Duration::from_secs(120);

/// Result of one finished process.
#[derive(Debug)]
pub struct Out {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
}

impl Out {
    /// Non-empty stderr lines.
    pub fn stderr_lines(&self) -> Vec<&str> {
        self.stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect()
    }
}

/// Runs `cmd` to completion, killing it at `limit`. Standard input is piped from `stdin` when
/// given; otherwise whatever the caller already set on `cmd` is kept (`Sandbox` commands start
/// with a null stdin).
pub fn run_with_watchdog(cmd: &mut Command, stdin: Option<&str>, limit: Duration) -> Out {
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = cmd.spawn().expect("spawn process");
    if let Some(text) = stdin {
        let mut pipe = child.stdin.take().expect("stdin piped");
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = pipe.write_all(text.as_bytes());
        });
    }
    let mut out_pipe = child.stdout.take().expect("stdout piped");
    let mut err_pipe = child.stderr.take().expect("stderr piped");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });
    let code = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status.code();
        }
        assert!(
            started.elapsed() < limit,
            "process still running after {limit:?}; killed by the test watchdog"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let elapsed = started.elapsed();
    Out {
        code,
        stdout: String::from_utf8_lossy(&out_reader.join().unwrap()).into_owned(),
        stderr: String::from_utf8_lossy(&err_reader.join().unwrap()).into_owned(),
        elapsed,
    }
}

/// Whether Node.js can run the scripts of this suite; hard-fails under CI, skips locally.
pub fn require_node(test_name: &str) -> bool {
    let available = Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        assert!(
            std::env::var_os("CI").is_none(),
            "{test_name}: node not found on PATH in CI"
        );
        eprintln!("{test_name}: skipped, node not found");
    }
    available
}

/// Whether `tsc` is on PATH; hard-fails under CI, skips locally.
pub fn require_tsc(test_name: &str) -> bool {
    let tsc = if cfg!(windows) { "tsc.cmd" } else { "tsc" };
    let available = Command::new(tsc)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !available {
        assert!(
            std::env::var_os("CI").is_none(),
            "{test_name}: tsc not found on PATH in CI"
        );
        eprintln!("{test_name}: skipped, tsc not found");
    }
    available
}

fn plain(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(stripped) if !stripped.starts_with(r"UNC\") => PathBuf::from(stripped),
        _ => path,
    }
}

/// Kills a child process on drop.
#[derive(Debug)]
pub struct KillOnDrop(pub Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the fixture over Streamable HTTP and returns the guard and its URL.
pub fn spawn_http_fixture(log: Option<&Path>) -> (KillOnDrop, String) {
    let mut cmd = Command::new(FIXTURE_BIN);
    cmd.args(["--http", "127.0.0.1:0"]).stdout(Stdio::piped());
    if let Some(log) = log {
        cmd.env("MCP_FIXTURE_LOG", log);
    }
    let mut child = KillOnDrop(cmd.spawn().expect("fixture must start"));
    let stdout = child.0.stdout.take().expect("stdout piped");
    let mut url = String::new();
    BufReader::new(stdout).read_line(&mut url).unwrap();
    (child, url.trim().to_string())
}

/// An isolated home, servers directory, skills directory and fixture call log.
#[derive(Debug)]
pub struct Sandbox {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub home: PathBuf,
    pub servers: PathBuf,
    pub skills: PathBuf,
    pub log: PathBuf,
}

impl Sandbox {
    /// Creates the directories (servers and skills under the default `~/.claude`) and an `mcp.json` registering the stdio fixture as [`SERVER`].
    ///
    /// The fixture gets a unique marker argument so tests can look for leftover processes.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = plain(dir.path().canonicalize().expect("canonicalize temp dir"));
        let sandbox = Self {
            home: root.join("home"),
            servers: root.join("home").join(".claude").join("servers"),
            skills: root.join("home").join(".claude").join("skills"),
            log: root.join("fixture.log"),
            root,
            _dir: dir,
        };
        std::fs::create_dir_all(sandbox.home.join(".claude")).unwrap();
        std::fs::create_dir_all(&sandbox.servers).unwrap();
        std::fs::create_dir_all(&sandbox.skills).unwrap();
        sandbox.write_mcp_json(&json!({ SERVER: sandbox.fixture_entry() }));
        sandbox
    }

    /// The marker argument carried by every fixture process this sandbox starts.
    pub fn marker(&self) -> String {
        format!("--sandbox-marker={}", self.root.display())
    }

    /// The `mcp.json` entry of the stdio fixture.
    pub fn fixture_entry(&self) -> Value {
        json!({
            "command": FIXTURE_BIN,
            "args": [self.marker()],
            "env": { "MCP_FIXTURE_LOG": self.log.display().to_string() }
        })
    }

    /// Replaces `~/.claude/mcp.json` with `servers`.
    pub fn write_mcp_json(&self, servers: &Value) {
        std::fs::write(
            self.home.join(".claude").join("mcp.json"),
            json!({ "mcpServers": servers }).to_string(),
        )
        .unwrap();
    }

    /// The CLI with every per-user location redirected into the sandbox.
    pub fn command(&self) -> Command {
        self.command_for(CLI_BIN)
    }

    /// `program` started with every per-user location redirected into the sandbox.
    pub fn command_for(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_STATE_HOME", self.home.join("state"))
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("XDG_DATA_HOME", self.home.join("data"))
            .env("LOCALAPPDATA", self.home.join("AppData").join("Local"))
            .env(mcp_execution_core::helper_protocol::CLI_PATH_ENV, CLI_BIN)
            .env_remove(mcp_execution_core::helper_protocol::HELPER_ENV)
            .env_remove(mcp_execution_core::helper_protocol::HELPER_TOKEN_ENV)
            .env_remove("RUST_LOG")
            .env_remove("RUST_BACKTRACE")
            .env_remove("RUST_LIB_BACKTRACE")
            .stdin(Stdio::null());
        cmd
    }

    /// Runs the CLI with `args`.
    pub fn cli(&self, args: &[&str]) -> Out {
        run_with_watchdog(self.command().args(args), None, WATCHDOG)
    }

    /// Runs the CLI with `args` and `stdin`.
    pub fn cli_stdin(&self, args: &[&str], stdin: &str) -> Out {
        run_with_watchdog(self.command().args(args), Some(stdin), WATCHDOG)
    }

    /// Runs `run <script> --servers-dir <sandbox servers> <extra...>`.
    pub fn run_script(&self, script: &Path, extra: &[&str]) -> Out {
        let script = script.display().to_string();
        let servers = self.servers.display().to_string();
        let mut args = vec!["run", script.as_str(), "--servers-dir", servers.as_str()];
        args.extend_from_slice(extra);
        self.cli(&args)
    }

    /// Generates the package of the `mcp.json` server `name` into the sandbox servers directory.
    pub fn generate_server(&self, name: &str) {
        let servers = self.servers.display().to_string();
        let out = self.cli(&[
            "generate",
            "--from-config",
            name,
            "--progressive-output",
            &servers,
        ]);
        assert_eq!(out.code, Some(0), "generate failed: {}", out.stderr);
        assert!(self.servers.join(name).join("index.ts").is_file());
    }

    /// Generates the stdio fixture package.
    pub fn generate(&self) {
        self.generate_server(SERVER);
    }

    /// Registers the HTTP fixture at `url` as `name` next to the stdio fixture.
    pub fn add_http_server(&self, name: &str, url: &str) {
        self.write_mcp_json(&json!({
            SERVER: self.fixture_entry(),
            name: { "type": "http", "url": url }
        }));
    }

    /// Writes `body` as `<name>` under the sandbox and returns its path.
    pub fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Forgets what the fixture logged so far (for example during `generate`).
    pub fn clear_fixture_log(&self) {
        let _ = std::fs::remove_file(&self.log);
    }

    /// Lines the fixture appended to its call log.
    pub fn fixture_log(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The runner's per-OS state root as seen by a CLI started with [`Sandbox::command`].
    pub fn state_root(&self) -> PathBuf {
        if cfg!(windows) {
            dirs::data_local_dir()
                .expect("data dir")
                .join("mcp-execution")
        } else if cfg!(target_os = "macos") {
            self.home
                .join("Library")
                .join("Application Support")
                .join("mcp-execution")
        } else {
            self.home.join("state").join("mcp-execution")
        }
    }

    /// Every regular file below `state_root()/<sub>`.
    pub fn state_files(&self, sub: &str) -> Vec<PathBuf> {
        files_below(&self.state_root().join(sub))
    }
}

/// Every regular file below `dir`, recursively.
pub fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_below(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// The token and the lines of a `run` stdout in text mode.
#[derive(Debug)]
pub struct Anchored<'a> {
    pub token: &'a str,
    pub lines: Vec<&'a str>,
}

/// Splits a `run` stdout into its anchor token and lines, asserting the first line is the
/// `[mcp-execution <token>] run started` anchor.
pub fn anchored(stdout: &str) -> Anchored<'_> {
    let lines: Vec<&str> = stdout.lines().collect();
    let first = lines.first().copied().unwrap_or_default();
    let token = first
        .strip_prefix("[mcp-execution ")
        .and_then(|rest| rest.strip_suffix("] run started"))
        .unwrap_or_else(|| panic!("first stdout line is not the run anchor: {stdout:?}"));
    assert!(
        token.len() >= 16,
        "token too short to be unguessable: {token}"
    );
    Anchored { token, lines }
}

impl Anchored<'_> {
    /// The trailer: a later line carrying the run token and `script_status`.
    pub fn trailer(&self) -> &str {
        self.lines
            .iter()
            .skip(1)
            .find(|l| l.contains(self.token) && l.contains("script_status"))
            .copied()
            .unwrap_or_else(|| panic!("no trailer with the token in {:?}", self.lines))
    }
}

/// Number of live processes whose command line contains `marker` (Unix only).
#[cfg(unix)]
pub fn processes_matching(marker: &str) -> usize {
    let out = Command::new("ps")
        .args(["-A", "-o", "args="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(marker))
        .count()
}

/// Polls until no process carries `marker` or `within` elapses; returns the final count.
#[cfg(unix)]
pub fn wait_no_processes(marker: &str, within: Duration) -> usize {
    let deadline = Instant::now() + within;
    loop {
        let count = processes_matching(marker);
        if count == 0 || Instant::now() >= deadline {
            return count;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A saved-script file in the ADR-579 format: a JSON header in a leading block comment, then
/// `body`.
pub fn script_text(name: &str, description: &str, tools: &[&str], body: &str) -> String {
    let header = json!({
        "v": 1,
        "name": name,
        "description": description,
        "parameters": [],
        "tools": tools,
        "writes": "read_only",
    });
    format!("/*! mcp-script\n{header}\n*/\n{body}\n")
}

/// Path and bytes of every regular file below `dir`.
pub fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    files_below(dir)
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

impl Sandbox {
    /// `<skills>/<skill>/scripts`, created.
    pub fn scripts_dir(&self, skill: &str) -> PathBuf {
        let dir = self.skills.join(skill).join("scripts");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes a saved script `<name>.ts` for `skill` and returns its path.
    pub fn saved_script(&self, skill: &str, name: &str, text: &str) -> PathBuf {
        let path = self.scripts_dir(skill).join(format!("{name}.ts"));
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Rewrites the generated `_meta.json` of `server` through `edit`.
    pub fn edit_meta(&self, server: &str, edit: impl FnOnce(&mut Value)) {
        let path = self.servers.join(server).join("_meta.json");
        let mut meta: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        edit(&mut meta);
        std::fs::write(&path, meta.to_string()).unwrap();
    }
}

/// Sets the modification time of `path` (a file or, on Unix, a directory) to `age_secs` ago.
#[cfg(unix)]
pub fn back_date(path: &Path, age_secs: u64) {
    let when = std::time::SystemTime::now() - Duration::from_secs(age_secs);
    std::fs::File::open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

/// A sparse regular file of `bytes` (apparent size) last modified `age_secs` ago.
#[cfg(unix)]
pub fn seed_file(dir: &Path, name: &str, bytes: u64, age_secs: u64) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::File::create(&path)
        .unwrap()
        .set_len(bytes)
        .unwrap();
    back_date(&path, age_secs);
    path
}

/// An empty directory `name` below `dir` last modified `age_secs` ago.
#[cfg(unix)]
pub fn seed_dir(dir: &Path, name: &str, age_secs: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    back_date(&path, age_secs);
    path
}

/// A directory below `root` that holds only a `node` link to the real binary, for tests that
/// restrict `PATH`.
///
/// A version-manager shim (mise, asdf, volta) only works under its own `PATH`, so the link
/// points at the binary the shim resolves to (`process.execPath`).
#[cfg(unix)]
pub fn node_only_dir(root: &Path) -> PathBuf {
    let reported = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .expect("run node");
    let node = PathBuf::from(String::from_utf8(reported.stdout).unwrap().trim());
    assert!(
        node.is_file(),
        "node reported a missing binary: {}",
        node.display()
    );
    let dir = root.join("only-node");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(&node, dir.join("node")).unwrap();
    dir
}
