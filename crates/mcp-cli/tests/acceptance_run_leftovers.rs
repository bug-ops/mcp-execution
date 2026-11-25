//! Acceptance test of the script process group after a normal exit (#656): a background process
//! the script left behind is stopped when the run ends, like on timeout and cancel.
//!
//! The script starts `sleep 300` with ignored stdio, records its pid, detaches it with
//! `child.unref()` (otherwise Node would wait for the child and the run would end only at its
//! timeout) and ends.
//!
//! Unix only: process groups. The Windows job object is covered by `acceptance_job_object`.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::time::Duration;

use common::{Sandbox, assert_gone, require_node, send_signal, wait_for_pid};

const GONE_WITHIN: Duration = Duration::from_secs(5);

const LEAVING_SCRIPT: &str = "import { spawn } from 'node:child_process';\n\
     import { writeFileSync } from 'node:fs';\n\
     const child = spawn('sleep', ['300'], { stdio: 'ignore' });\n\
     writeFileSync(process.argv[2], String(child.pid));\n\
     child.unref();\n";

/// Kills the leftover when the test ends, whatever the outcome.
struct Cleanup(u32);

impl Drop for Cleanup {
    fn drop(&mut self) {
        send_signal("KILL", &self.0.to_string());
    }
}

#[test]
fn run_stops_a_background_process_the_script_left_after_a_normal_exit() {
    if !require_node("run_stops_a_background_process_the_script_left_after_a_normal_exit") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("leave.mts", LEAVING_SCRIPT);
    let pid_file = sb.root.join("leftover.pid");

    let out = sb.run_script(
        &script,
        &["--timeout", "30", "--", &pid_file.display().to_string()],
    );

    let leftover = wait_for_pid(
        &pid_file,
        Duration::from_secs(1),
        "the script never reported its leftover",
    );
    let _cleanup = Cleanup(leftover);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_gone(leftover, GONE_WITHIN, "the process the script left behind");
}
