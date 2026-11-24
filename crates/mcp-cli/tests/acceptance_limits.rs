//! Acceptance tests of the on-disk bounds of `run` (FR-036.13): the spill file size, count, total
//! and age limits, and the pruning of per-run directories by age and by count.
//!
//! The state directory is seeded before the run (sparse files and back-dated modification times)
//! and inspected after it. Unix only: the state root of a Windows run cannot be redirected and
//! back-dating a directory needs a Unix file handle.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::path::PathBuf;

use common::{Sandbox, files_below, require_node, seed_dir, seed_file};
use mcp_execution_cli::runtime::dirs::{MAX_RUN_DIRS, RUN_DIR_COUNT_PRUNE_MIN_AGE};
use mcp_execution_cli::runtime::spill::{
    MAX_SPILL_FILE_BYTES, MAX_SPILL_FILES, MAX_SPILL_TOTAL_BYTES,
};

const HOUR: u64 = 3600;

/// Runs a script that spills (stdout over a 100 byte budget).
fn spilling_run(sb: &Sandbox) -> common::Out {
    let script = sb.script(
        "spill.ts",
        "process.stdout.write('0123456789'.repeat(100));\n",
    );
    let out = sb.run_script(&script, &["--max-output", "100"]);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    out
}

fn sandbox(test: &str) -> Option<Sandbox> {
    require_node(test).then(Sandbox::new)
}

#[test]
fn spill_file_is_capped_at_eight_mebibytes_and_the_run_still_succeeds() {
    let Some(sb) = sandbox("spill_file_is_capped_at_eight_mebibytes_and_the_run_still_succeeds")
    else {
        return;
    };
    let script = sb.script(
        "huge.ts",
        &format!(
            "process.stdout.write('x'.repeat({}));\n",
            MAX_SPILL_FILE_BYTES + 1024 * 1024
        ),
    );

    let out = sb.run_script(&script, &["--max-output", "100", "--timeout", "60"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let largest = sb
        .state_files("spill")
        .iter()
        .map(|f| std::fs::metadata(f).unwrap().len())
        .max()
        .expect("a spill file");
    assert_eq!(largest, MAX_SPILL_FILE_BYTES as u64);
    let lower = out.stdout.to_lowercase();
    assert!(
        lower.contains("truncat") || lower.contains("limit reached"),
        "the notice must say the spill was cut: {}",
        out.stdout
    );
}

#[test]
fn spill_keeps_at_most_the_file_count_limit_dropping_the_oldest() {
    let Some(sb) = sandbox("spill_keeps_at_most_the_file_count_limit_dropping_the_oldest") else {
        return;
    };
    let spill = sb.state_root().join("spill");
    let seeded: Vec<PathBuf> = (1..=40)
        .map(|n| seed_file(&spill, &format!("seed-{n:02}.txt"), 10, n * HOUR / 10))
        .collect();

    spilling_run(&sb);

    assert!(
        files_below(&spill).len() <= MAX_SPILL_FILES,
        "{} files remain",
        files_below(&spill).len()
    );
    assert!(seeded[0].exists(), "the newest seeded file must survive");
    assert!(
        !seeded[39].exists(),
        "the oldest seeded file must be pruned"
    );
}

#[test]
fn spill_keeps_the_total_size_within_the_limit() {
    let Some(sb) = sandbox("spill_keeps_the_total_size_within_the_limit") else {
        return;
    };
    let spill = sb.state_root().join("spill");
    for n in 1..=12 {
        seed_file(
            &spill,
            &format!("seed-{n:02}.txt"),
            MAX_SPILL_FILE_BYTES as u64,
            n * HOUR / 10,
        );
    }

    spilling_run(&sb);

    let total: u64 = files_below(&spill)
        .iter()
        .map(|f| std::fs::metadata(f).unwrap().len())
        .sum();
    assert!(
        total <= MAX_SPILL_TOTAL_BYTES,
        "{total} bytes remain, limit {MAX_SPILL_TOTAL_BYTES}"
    );
}

#[test]
fn spill_removes_files_older_than_seven_days_and_keeps_younger_ones() {
    let Some(sb) = sandbox("spill_removes_files_older_than_seven_days_and_keeps_younger_ones")
    else {
        return;
    };
    let spill = sb.state_root().join("spill");
    let expired = seed_file(&spill, "seed-expired.txt", 10, 8 * 24 * HOUR);
    let young = seed_file(&spill, "seed-young.txt", 10, 6 * 24 * HOUR);

    spilling_run(&sb);

    assert!(!expired.exists(), "an 8 day old spill file must be removed");
    assert!(young.exists(), "a 6 day old spill file must be kept");
}

#[test]
fn run_dirs_older_than_a_day_are_pruned_and_younger_ones_kept() {
    let Some(sb) = sandbox("run_dirs_older_than_a_day_are_pruned_and_younger_ones_kept") else {
        return;
    };
    let runs = sb.state_root().join("runs");
    let old = seed_dir(&runs, "seed-old", 25 * HOUR);
    let recent = seed_dir(&runs, "seed-recent", HOUR);

    spilling_run(&sb);

    assert!(!old.exists(), "a 25 hour old run dir must be removed");
    assert!(recent.exists(), "a 1 hour old run dir must be kept");
}

#[test]
fn run_dirs_over_the_count_limit_are_pruned_oldest_first() {
    let Some(sb) = sandbox("run_dirs_over_the_count_limit_are_pruned_oldest_first") else {
        return;
    };
    let runs = sb.state_root().join("runs");
    let past_the_guard = RUN_DIR_COUNT_PRUNE_MIN_AGE.as_secs() * 2;
    let seeded: Vec<PathBuf> = (0..40)
        .map(|n| seed_dir(&runs, &format!("seed-{n:02}"), past_the_guard + n))
        .collect();

    spilling_run(&sb);

    let remaining = seeded.iter().filter(|p| p.exists()).count();
    assert!(
        remaining <= MAX_RUN_DIRS,
        "{remaining} seeded run dirs remain, limit {MAX_RUN_DIRS}"
    );
    assert!(seeded[0].exists(), "the newest must survive");
    assert!(!seeded[39].exists(), "the oldest must be pruned");
}

#[test]
fn run_dirs_younger_than_the_guard_age_are_never_pruned_by_count() {
    let Some(sb) = sandbox("run_dirs_younger_than_the_guard_age_are_never_pruned_by_count") else {
        return;
    };
    let runs = sb.state_root().join("runs");
    let seeded: Vec<PathBuf> = (0..40)
        .map(|n| seed_dir(&runs, &format!("seed-{n:02}"), 30 + n))
        .collect();

    spilling_run(&sb);

    assert!(
        seeded.iter().all(|p| p.exists()),
        "a concurrent live run's directory must never be pruned"
    );
}
