//! Acceptance tests of the reserved-name rule of `ServerSlug` (#696): a Windows reserved device
//! name cannot be used as a directory name, so it is not a slug.

use mcp_execution_core::{ServerIdSlugError, ServerSlug};

fn reserved_names() -> Vec<String> {
    let mut names: Vec<String> = ["con", "prn", "aux", "nul"].map(String::from).into();
    for digit in 0..=9 {
        names.push(format!("com{digit}"));
        names.push(format!("lpt{digit}"));
    }
    names
}

#[test]
fn new_rejects_every_windows_reserved_device_name() {
    let names = reserved_names();
    assert_eq!(names.len(), 24);

    for name in names {
        let error = ServerSlug::new(&name).expect_err(&name);

        assert_eq!(error, ServerIdSlugError::ReservedName, "{name}");
        assert_eq!(
            error.to_string(),
            "server_id must not be a Windows reserved device name"
        );
    }
}

#[test]
fn deserialize_rejects_every_windows_reserved_device_name() {
    for name in reserved_names() {
        let parsed = serde_json::from_str::<ServerSlug>(&format!("\"{name}\""));

        assert!(parsed.is_err(), "{name} was accepted: {parsed:?}");
    }
}

/// Guard against an over-wide rule: names that only resemble a device name stay valid.
#[test]
fn new_accepts_names_that_only_resemble_a_reserved_name() {
    for name in ["con-1", "nul2", "com10", "lpt", "com", "console", "a-aux"] {
        assert!(ServerSlug::new(name).is_ok(), "{name}");
    }
    assert_eq!(
        ServerSlug::new("CON").unwrap_err(),
        ServerIdSlugError::InvalidCharacters
    );
}
