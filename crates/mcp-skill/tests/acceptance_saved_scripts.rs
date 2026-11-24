//! Acceptance tests of saved scripts (#579, ADR-579, FR-044): `RecipeName` validation, `save`
//! with header validation, exact text, overwrite and size rules and confinement, `list`
//! signatures and `check` against the recorded server metadata.
//!
//! Absent on purpose: "`list` prints no body" (`ScriptListing` has no body field, so the type
//! proves it).
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::path::Path;

use mcp_execution_core::metadata::{
    METADATA_SCHEMA_VERSION, ResultTypeOrigin, ServerMetadata, ToolMetadata,
};
use mcp_execution_core::provenance::GenerationProvenance;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_skill::scripts::{
    Overwrite, RecipeName, ScriptError, WriteIntent, check, list, save,
};

fn script_text(name: &str, tools: &[&str], body: &str) -> String {
    let header = serde_json::json!({
        "v": 1,
        "name": name,
        "description": format!("Script {name}"),
        "parameters": [],
        "tools": tools,
        "writes": "read_only",
    });
    format!("/*! mcp-script\n{header}\n*/\n{body}\n")
}

fn recipe(name: &str) -> RecipeName {
    RecipeName::new(name).unwrap()
}

fn scripts_dir(skill: &Path) -> std::path::PathBuf {
    skill.join("scripts")
}

fn files_in(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn metadata_with(tools: &[&str]) -> ServerMetadata {
    let config = ServerConfig::builder()
        .command("docker".to_string())
        .build()
        .unwrap();
    ServerMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        server_id: ServerId::new("fixture").unwrap(),
        server_name: "Fixture".to_string(),
        server_version: "1.0.0".to_string(),
        tools: tools
            .iter()
            .map(|name| ToolMetadata {
                name: ToolName::new(*name).unwrap(),
                typescript_name: (*name).to_string(),
                category: None,
                keywords: vec![],
                description: None,
                parameters: vec![],
                annotations: None,
                result_type: None,
                result_type_origin: ResultTypeOrigin::Undeclared,
                manifest_size_bytes: 0,
            })
            .collect(),
        provenance: GenerationProvenance::capture(&config, &[]),
    }
}

#[test]
fn recipe_name_accepts_a_slug_and_rejects_separators_case_and_unbounded_length() {
    assert!(RecipeName::new("triage-issues").is_ok());
    for bad in [
        "",
        "../escape",
        "a/b",
        "a\\b",
        "Has Space",
        "UPPER",
        ".hidden",
        "name.ts",
    ] {
        assert!(
            matches!(RecipeName::new(bad), Err(ScriptError::InvalidName)),
            "{bad:?} must be rejected"
        );
    }
    assert!(matches!(
        RecipeName::new(&"a".repeat(1000)),
        Err(ScriptError::InvalidName)
    ));
}

#[tokio::test]
async fn save_stores_the_text_exactly_under_the_confined_scripts_directory() {
    let skill = tempfile::tempdir().unwrap();
    let text = script_text(
        "triage",
        &["fixture/list_items"],
        "const x = 1;  // keep   spacing",
    );

    let path = save(skill.path(), &recipe("triage"), &text, Overwrite::Refuse)
        .await
        .unwrap();

    assert_eq!(path, scripts_dir(skill.path()).join("triage.ts"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[tokio::test]
async fn save_with_an_invalid_header_writes_nothing() {
    let skill = tempfile::tempdir().unwrap();
    let bad_texts = [
        "console.log('no header');\n".to_string(),
        "/*! mcp-script\n{not json}\n*/\nbody\n".to_string(),
        "/*! mcp-script\n{\"v\":1,\"name\":\"triage\"}\n*/\nbody\n".to_string(),
    ];

    for text in bad_texts {
        let result = save(skill.path(), &recipe("triage"), &text, Overwrite::Refuse).await;

        assert!(
            matches!(result, Err(ScriptError::InvalidHeader { .. })),
            "{text:?} -> {result:?}"
        );
    }
    assert_eq!(
        files_in(&scripts_dir(skill.path())),
        Vec::<std::path::PathBuf>::new()
    );
}

#[tokio::test]
async fn save_refuses_a_header_name_that_differs_from_the_recipe_name() {
    let skill = tempfile::tempdir().unwrap();
    let text = script_text("header-name", &["fixture/echo"], "export {};");

    let result = save(skill.path(), &recipe("file-name"), &text, Overwrite::Refuse).await;

    assert!(
        matches!(result, Err(ScriptError::InvalidHeader { .. })),
        "{result:?}"
    );
    assert_eq!(
        files_in(&scripts_dir(skill.path())),
        Vec::<std::path::PathBuf>::new()
    );
}

#[tokio::test]
async fn save_refuses_an_existing_name_unless_replace_is_requested() {
    let skill = tempfile::tempdir().unwrap();
    let first = script_text("again", &["fixture/echo"], "// first");
    let second = script_text("again", &["fixture/echo"], "// second");
    let path = save(skill.path(), &recipe("again"), &first, Overwrite::Refuse)
        .await
        .unwrap();

    let refused = save(skill.path(), &recipe("again"), &second, Overwrite::Refuse).await;

    assert!(
        matches!(refused, Err(ScriptError::AlreadyExists { .. })),
        "{refused:?}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    save(skill.path(), &recipe("again"), &second, Overwrite::Replace)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), second);
}

#[tokio::test]
async fn save_rejects_a_script_over_the_size_bound_and_stops_at_a_script_count_bound() {
    let skill = tempfile::tempdir().unwrap();
    let huge = script_text("huge", &["fixture/echo"], &"x".repeat(1024 * 1024));

    let too_large = save(skill.path(), &recipe("huge"), &huge, Overwrite::Refuse).await;

    assert!(
        matches!(too_large, Err(ScriptError::TooLarge { .. })),
        "{too_large:?}"
    );
    let mut saved = 0_usize;
    let mut refused = false;
    for index in 0..200 {
        let name = format!("s-{index:03}");
        let text = script_text(&name, &["fixture/echo"], "export {};");
        if save(skill.path(), &recipe(&name), &text, Overwrite::Refuse)
            .await
            .is_ok()
        {
            saved += 1;
        } else {
            refused = true;
            break;
        }
    }
    assert!(refused, "the number of scripts per skill must be bounded");
    assert!(
        saved >= 10,
        "the bound must allow a useful number of scripts, got {saved}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn save_rejects_a_symlinked_scripts_directory() {
    let skill = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), scripts_dir(skill.path())).unwrap();
    let text = script_text("escape", &["fixture/echo"], "export {};");

    let result = save(skill.path(), &recipe("escape"), &text, Overwrite::Refuse).await;

    assert!(
        matches!(result, Err(ScriptError::NotConfined)),
        "{result:?}"
    );
    assert!(
        files_in(outside.path()).is_empty(),
        "nothing may land outside the skill"
    );
}

#[tokio::test]
async fn list_returns_one_signature_per_valid_script_and_skips_an_unreadable_header() {
    let skill = tempfile::tempdir().unwrap();
    save(
        skill.path(),
        &recipe("alpha"),
        &script_text("alpha", &["fixture/list_items", "fixture/echo"], "// body"),
        Overwrite::Refuse,
    )
    .await
    .unwrap();
    std::fs::write(
        scripts_dir(skill.path()).join("broken.ts"),
        "no header here\n",
    )
    .unwrap();

    let listings = list(skill.path()).await.unwrap();

    assert_eq!(listings.len(), 1, "{listings:?}");
    let alpha = &listings[0];
    assert_eq!(alpha.name.as_str(), "alpha");
    assert_eq!(alpha.description, "Script alpha");
    assert_eq!(alpha.writes, WriteIntent::ReadOnly);
    let tools: Vec<String> = alpha
        .tools
        .iter()
        .map(|t| format!("{}/{}", t.server, t.tool))
        .collect();
    assert_eq!(tools, ["fixture/list_items", "fixture/echo"]);
}

#[tokio::test]
async fn check_reports_missing_tools_and_invalid_scripts_and_modifies_nothing() {
    let skill = tempfile::tempdir().unwrap();
    for (name, tools) in [("good", "fixture/echo"), ("stale", "fixture/removed_tool")] {
        save(
            skill.path(),
            &recipe(name),
            &script_text(name, &[tools], "export {};"),
            Overwrite::Refuse,
        )
        .await
        .unwrap();
    }
    std::fs::write(scripts_dir(skill.path()).join("headerless.ts"), "oops\n").unwrap();
    let before: Vec<_> = {
        let mut files = files_in(&scripts_dir(skill.path()));
        files.sort();
        files
            .into_iter()
            .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
            .collect()
    };

    let report = check(skill.path(), &metadata_with(&["echo", "list_items"])).await;

    assert!(!report.is_clean());
    assert_eq!(report.missing_tools.len(), 1, "{report:?}");
    assert_eq!(report.missing_tools[0].script.as_str(), "stale");
    assert_eq!(report.missing_tools[0].tool.tool.as_str(), "removed_tool");
    assert_eq!(report.invalid_scripts.len(), 1, "{report:?}");
    assert!(report.invalid_scripts[0].contains("headerless"));
    let mut after = files_in(&scripts_dir(skill.path()));
    after.sort();
    let after: Vec<_> = after
        .into_iter()
        .map(|p| (p.clone(), std::fs::read(&p).unwrap()))
        .collect();
    assert_eq!(after, before, "check must not modify anything");
}

#[tokio::test]
async fn check_of_sound_scripts_is_clean() {
    let skill = tempfile::tempdir().unwrap();
    save(
        skill.path(),
        &recipe("good"),
        &script_text("good", &["fixture/echo"], "export {};"),
        Overwrite::Refuse,
    )
    .await
    .unwrap();

    let report = check(skill.path(), &metadata_with(&["echo"])).await;

    assert!(report.is_clean(), "{report:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_saves_of_one_name_have_exactly_one_winner_and_a_whole_file() {
    for round in 0..25 {
        let skill = tempfile::tempdir().unwrap();
        let name = recipe("race");
        let texts = [
            script_text("race", &["fixture/echo"], &format!("// first {round}")),
            script_text("race", &["fixture/echo"], &format!("// second {round}")),
        ];
        let tasks: Vec<_> = texts
            .iter()
            .cloned()
            .map(|text| {
                let dir = skill.path().to_path_buf();
                let name = name.clone();
                tokio::spawn(async move { save(&dir, &name, &text, Overwrite::Refuse).await })
            })
            .collect();

        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await.unwrap());
        }

        let winners = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(winners, 1, "round {round}: {results:?}");
        assert!(
            results
                .iter()
                .any(|r| matches!(r, Err(ScriptError::AlreadyExists { .. }))),
            "round {round}: the loser must see the existing name: {results:?}"
        );
        let stored = std::fs::read_to_string(scripts_dir(skill.path()).join("race.ts")).unwrap();
        let winner = results.iter().position(Result::is_ok).unwrap();
        assert_eq!(
            stored, texts[winner],
            "round {round}: the stored file must be the winner's text"
        );
        assert_eq!(
            files_in(&scripts_dir(skill.path())).len(),
            1,
            "round {round}: leftover files"
        );
    }
}

#[tokio::test]
async fn a_write_that_fails_leaves_no_temporary_file_behind() {
    let skill = tempfile::tempdir().unwrap();
    let blocker = scripts_dir(skill.path()).join("blocked.ts");
    std::fs::create_dir_all(&blocker).unwrap();
    let text = script_text("blocked", &["fixture/echo"], "export {};");

    let result = save(skill.path(), &recipe("blocked"), &text, Overwrite::Replace).await;

    assert!(result.is_err(), "a directory in the way must fail the save");
    assert_eq!(
        files_in(&scripts_dir(skill.path())),
        vec![blocker],
        "only the blocking directory may remain, no temporary file"
    );
}
