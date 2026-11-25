//! Acceptance tests of `setup` without Node.js (#685): Bun, then Deno, is probed with a typed
//! script from a private directory; the first success is reported with a caveat.
//!
//! Fake runtimes are `sh` scripts on a `PATH` that holds no Node.js. Unix only.

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use common::{
    HangingNode, KillOnDrop, Out, Sandbox, WATCHDOG, assert_gone, require_alt_runtime,
    require_node, run_with_watchdog, send_signal, wait_for_exit,
};
use serde_json::{Value, json};

const CAVEAT: &str = "run, --typecheck and --restrict need Node.js 22.18.0 (23.6.0 on the 23 line)";

struct Fakes {
    sb: Sandbox,
    bin: PathBuf,
    cwd: PathBuf,
}

impl Fakes {
    fn new() -> Self {
        let sb = Sandbox::new();
        let bin = sb.root.join("fakebin");
        let cwd = sb.root.join("checkout");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        Self { sb, bin, cwd }
    }

    /// Installs `program` as a script that records its working directory, arguments and the probe
    /// file (the last argument) in `<program>.log`, then runs `body`.
    fn install_in(&self, dir: &Path, program: &str, body: &str) {
        let log = self.sb.root.join(format!("{program}.log"));
        let text = format!(
            "#!/bin/sh\n\
             for last; do :; done\n\
             {{ printf 'cwd=%s\\n' \"$(pwd -P)\"; printf 'args=%s\\n' \"$*\"; \
             printf 'probe=%s\\n' \"$last\"; /bin/cat \"$last\"; }} > '{}'\n\
             {body}\n",
            log.display()
        );
        let path = dir.join(program);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn install(&self, program: &str, body: &str) {
        self.install_in(&self.bin, program, body);
    }

    fn log(&self, program: &str) -> Option<String> {
        std::fs::read_to_string(self.sb.root.join(format!("{program}.log"))).ok()
    }

    fn setup_with_path(&self, path: &str, flags: &[&str]) -> Out {
        let mut cmd = self.sb.command();
        cmd.env("PATH", path)
            .current_dir(&self.cwd)
            .args(flags)
            .arg("setup");
        run_with_watchdog(&mut cmd, None, WATCHDOG)
    }

    fn setup(&self, flags: &[&str]) -> Out {
        self.setup_with_path(&self.bin.display().to_string(), flags)
    }

    fn setup_json(&self) -> Value {
        parsed(&self.setup(&["--format", "json"]))
    }
}

fn parsed(out: &Out) -> Value {
    assert_eq!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
    serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("not JSON ({e}): {}", out.stdout))
}

fn prints(version: &str) -> String {
    format!("echo 'mcp-execution-probe {version}'")
}

fn logged<'a>(log: &'a str, key: &str) -> &'a str {
    log.lines()
        .find_map(|line| line.strip_prefix(key))
        .unwrap_or_else(|| panic!("no {key} in {log}"))
}

#[test]
fn setup_without_node_reports_bun_with_the_caveat() {
    let fakes = Fakes::new();
    fakes.install("bun", &prints("1.3.0"));

    let result = fakes.setup_json();

    assert_eq!(
        result["runtime"],
        json!({ "name": "bun", "version": "1.3.0" }),
        "{result}"
    );
    assert_eq!(result["caveat"], CAVEAT, "{result}");
    assert!(result.get("node_version").is_none(), "{result}");
}

#[test]
fn setup_pretty_output_shows_the_caveat_and_the_bun_invocation() {
    let fakes = Fakes::new();
    fakes.install("bun", &prints("1.3.0"));

    let out = fakes.setup(&[]);

    assert_eq!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
    assert!(out.stdout.contains(CAVEAT), "{}", out.stdout);
    assert!(
        out.stdout
            .lines()
            .any(|line| line.trim_start().starts_with("bun ") && line.contains(".ts")),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("node ~/.claude"), "{}", out.stdout);
}

#[test]
fn setup_probes_deno_without_config_when_bun_is_absent() {
    let fakes = Fakes::new();
    fakes.install("deno", &prints("2.5.0"));

    let out = fakes.setup(&[]);
    let result = fakes.setup_json();

    assert_eq!(
        result["runtime"],
        json!({ "name": "deno", "version": "2.5.0" }),
        "{result}"
    );
    assert!(out.stdout.contains("deno run -A "), "{}", out.stdout);
    let log = fakes.log("deno").unwrap();
    assert_eq!(
        logged(&log, "args="),
        format!("run --no-config --quiet {}", logged(&log, "probe=")),
    );
}

#[test]
fn setup_prefers_bun_over_deno() {
    let fakes = Fakes::new();
    fakes.install("bun", &prints("1.3.0"));
    fakes.install("deno", &prints("2.5.0"));

    let result = fakes.setup_json();

    assert_eq!(result["runtime"]["name"], "bun", "{result}");
    assert!(fakes.log("deno").is_none(), "deno was probed too");
}

#[test]
fn setup_falls_back_to_the_next_runtime_when_a_probe_fails() {
    for body in [
        "exit 1",
        "echo 'something else'",
        "echo 'mcp-execution-probe 1.0'; exit 3",
    ] {
        let fakes = Fakes::new();
        fakes.install("bun", body);
        fakes.install("deno", &prints("2.5.0"));

        let result = fakes.setup_json();

        assert_eq!(result["runtime"]["name"], "deno", "{body}: {result}");
        assert!(fakes.log("bun").is_some(), "{body}: bun was not probed");
    }
}

#[test]
fn setup_probes_from_a_private_directory_with_a_typed_script_and_removes_it() {
    let fakes = Fakes::new();
    std::fs::write(fakes.cwd.join("bunfig.toml"), "preload = [\"./evil.ts\"]\n").unwrap();
    fakes.install("bun", &prints("1.3.0"));

    fakes.setup_json();

    let log = fakes.log("bun").unwrap();
    let probe = PathBuf::from(logged(&log, "probe="));
    let cwd = PathBuf::from(logged(&log, "cwd="));
    assert_ne!(cwd, fakes.cwd, "the probe ran in the caller's directory");
    assert_eq!(
        cwd.file_name(),
        probe.parent().unwrap().file_name(),
        "the probe must run in its own directory: {log}"
    );
    assert!(!cwd.join("bunfig.toml").exists(), "{log}");
    assert!(log.contains(": string"), "the probe is not typed: {log}");
    assert!(!probe.exists(), "the probe file was left behind: {log}");
}

#[test]
fn setup_bounds_the_reported_version_length() {
    let fakes = Fakes::new();
    fakes.install("bun", &prints(&"9".repeat(300)));

    let result = fakes.setup_json();

    let version = result["runtime"]["version"].as_str().unwrap();
    assert!((1..=64).contains(&version.len()), "{version}");
}

#[test]
fn setup_ignores_relative_path_entries() {
    let fakes = Fakes::new();
    std::fs::create_dir_all(fakes.cwd.join("relbin")).unwrap();
    fakes.install_in(&fakes.cwd.join("relbin"), "bun", &prints("6.6.6"));
    fakes.install("deno", &prints("2.5.0"));

    let path = format!("relbin:{}", fakes.bin.display());
    let result = parsed(&fakes.setup_with_path(&path, &["--format", "json"]));

    assert_eq!(result["runtime"]["name"], "deno", "{result}");
    assert!(fakes.log("bun").is_none(), "a relative PATH entry was run");
}

#[test]
fn setup_without_any_runtime_fails_and_names_bun_and_deno() {
    let fakes = Fakes::new();

    let out = fakes.setup(&[]);

    assert_ne!(out.code, Some(0), "{}", out.stdout);
    let text = format!("{} {}", out.stdout, out.stderr);
    assert!(text.contains("Bun") && text.contains("Deno"), "{text}");
}

/// Takes the 10 s probe timeout.
#[test]
fn setup_times_out_a_hanging_probe_and_removes_the_probe_file() {
    let fakes = Fakes::new();
    fakes.install("bun", "exec /bin/sleep 60");

    let out = fakes.setup(&[]);

    assert_ne!(out.code, Some(0), "{}", out.stdout);
    assert!(
        out.elapsed < Duration::from_secs(40),
        "took {:?}",
        out.elapsed
    );
    let log = fakes.log("bun").unwrap();
    assert!(
        !Path::new(logged(&log, "probe=")).exists(),
        "the probe file was left behind: {log}"
    );
}

#[test]
fn setup_with_node_reports_node_without_a_caveat() {
    if !require_node("setup_with_node_reports_node_without_a_caveat") {
        return;
    }
    let sb = Sandbox::new();

    let result = parsed(&sb.cli(&["--format", "json", "setup"]));

    assert_eq!(result["runtime"]["name"], "node", "{result}");
    assert!(result["runtime"]["version"].is_string(), "{result}");
    assert!(result["caveat"].is_null(), "{result}");
    assert!(result.get("node_version").is_none(), "{result}");
}

/// The real binary of `program`, since a version-manager shim only works under its own `PATH`.
fn real_binary(program: &str, args: &[&str]) -> PathBuf {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .unwrap();
    let path = PathBuf::from(String::from_utf8(out.stdout).unwrap().trim());
    assert!(path.is_file(), "{program} reported {}", path.display());
    path
}

fn assert_setup_accepts_the_real(program: &str, args: &[&str]) {
    if !require_alt_runtime(program, &format!("setup_accepts_the_real_{program}")) {
        return;
    }
    let fakes = Fakes::new();
    std::os::unix::fs::symlink(real_binary(program, args), fakes.bin.join(program)).unwrap();

    let result = fakes.setup_json();

    assert_eq!(result["runtime"]["name"], program, "{result}");
    assert_eq!(result["caveat"], CAVEAT, "{result}");
}

#[test]
fn setup_accepts_the_real_bun() {
    assert_setup_accepts_the_real("bun", &["-p", "process.execPath"]);
}

#[test]
fn setup_accepts_the_real_deno() {
    assert_setup_accepts_the_real("deno", &["eval", "console.log(Deno.execPath())"]);
}

/// Takes the 10 s Node.js version timeout.
#[test]
fn setup_gives_up_on_a_hanging_node_falls_back_to_bun_and_leaves_no_process() {
    let fakes = Fakes::new();
    let node = HangingNode::install(&fakes.bin);
    fakes.install("bun", &prints("1.3.0"));

    let out = fakes.setup(&["--format", "json"]);

    assert!(
        out.elapsed < Duration::from_secs(40),
        "took {:?}",
        out.elapsed
    );
    let result = parsed(&out);
    assert_eq!(result["runtime"]["name"], "bun", "{result}");
    assert_gone(
        node.child_pid(),
        Duration::from_secs(5),
        "the child of the node shim",
    );
}

/// Takes the 10 s Node.js version timeout.
#[test]
fn setup_names_the_node_timeout_when_no_other_runtime_exists() {
    let fakes = Fakes::new();
    let _node = HangingNode::install(&fakes.bin);

    let out = fakes.setup(&[]);

    assert_ne!(out.code, Some(0), "{}", out.stdout);
    assert!(
        out.stderr
            .contains("Node.js did not answer `node --version` within 10 s"),
        "{}",
        out.stderr
    );
}

#[test]
fn setup_cancelled_during_the_node_check_leaves_no_process_of_the_node_tree() {
    let fakes = Fakes::new();
    let node = HangingNode::install(&fakes.bin);
    let mut cmd = fakes.sb.command();
    cmd.env("PATH", &fakes.bin)
        .current_dir(&fakes.cwd)
        .arg("setup")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut setup = KillOnDrop(cmd.spawn().unwrap());
    let pid = node.child_pid();

    send_signal("TERM", &setup.0.id().to_string());
    let code = wait_for_exit(&mut setup.0, Duration::from_secs(8), "setup did not stop");

    assert_eq!(code, Some(143));
    assert_gone(pid, Duration::from_secs(5), "the child of the node shim");
}
