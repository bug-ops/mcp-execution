//! Documentation drift check for #607: the contributor instructions under `.github` must not pin
//! dependency versions or carry sample code for APIs that do not exist.
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::path::{Path, PathBuf};

/// Substrings the contributor instructions must not contain, each with the reason.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "Client::new(transport)",
        "no such rmcp constructor in this workspace; link a real doctest instead",
    ),
    (
        "ConnectionConfig",
        "type of the removed design; it exists in no crate",
    ),
    (
        "tokio = { version",
        "a pinned tokio version drifts; point to the workspace Cargo.toml",
    ),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn instruction_files() -> Vec<PathBuf> {
    let github = workspace_root().join(".github");
    let mut files = vec![github.join("copilot-instructions.md")];
    for entry in std::fs::read_dir(github.join("instructions")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    files
}

/// Whether `text` contains `rmcp` followed by a space and a digit (a pinned version).
fn pins_an_rmcp_version(text: &str) -> bool {
    text.match_indices("rmcp ").any(|(at, word)| {
        text[at + word.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

#[test]
fn contributor_instructions_carry_no_pinned_rmcp_version() {
    for file in instruction_files() {
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            !pins_an_rmcp_version(&text),
            "{} pins an rmcp version; say \"see the workspace Cargo.toml\"",
            file.display()
        );
    }
}

#[test]
fn contributor_instructions_carry_no_invented_samples_or_pinned_tokio() {
    for file in instruction_files() {
        let text = std::fs::read_to_string(&file).unwrap();
        for (needle, reason) in FORBIDDEN {
            assert!(
                !text.contains(needle),
                "{} contains {needle:?}: {reason}",
                file.display()
            );
        }
    }
}
