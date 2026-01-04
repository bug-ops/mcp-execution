//! Security edge case tests for CLI types.
//!
//! These tests verify that unicode bypass vectors, encoding tricks,
//! and other edge cases are properly handled by the validation logic.

use mcp_execution_core::cli::ServerConnectionString;

/// Test that zero-width Unicode characters are rejected.
#[test]
#[allow(clippy::similar_names)]
fn test_zero_width_unicode_rejected() {
    // Zero-width joiner
    let zwj = "server\u{200D}malicious";
    assert!(
        ServerConnectionString::new(zwj).is_err(),
        "Zero-width joiner should be rejected"
    );

    // Zero-width space
    let zws = "server\u{200B}malicious";
    assert!(
        ServerConnectionString::new(zws).is_err(),
        "Zero-width space should be rejected"
    );

    // Zero-width non-joiner
    let zwnj = "server\u{200C}evil";
    assert!(
        ServerConnectionString::new(zwnj).is_err(),
        "Zero-width non-joiner should be rejected"
    );
}

/// Test that bidirectional text override is rejected.
#[test]
fn test_bidi_override_rejected() {
    // Right-to-left override (visual spoofing attack)
    let bidi = "server\u{202E}evil";
    assert!(
        ServerConnectionString::new(bidi).is_err(),
        "Bidi override should be rejected"
    );
}

/// Test that URL encoding doesn't bypass validation.
#[test]
fn test_url_encoding_rejected() {
    // %26 = &
    let url_encoded = "server%26%26rm";
    assert!(
        ServerConnectionString::new(url_encoded).is_err(),
        "URL encoded characters should be rejected"
    );

    // %2F = /
    let slash = "server%2Fetc%2Fpasswd";
    assert!(
        ServerConnectionString::new(slash).is_err(),
        "URL encoded slashes should be rejected"
    );
}

/// Test that hex/octal escapes don't bypass validation.
#[test]
fn test_escape_sequences_rejected() {
    // Backslash-based escapes
    let hex = "server\\x26\\x26";
    assert!(
        ServerConnectionString::new(hex).is_err(),
        "Hex escapes should be rejected"
    );

    let octal = "server\\046\\046";
    assert!(
        ServerConnectionString::new(octal).is_err(),
        "Octal escapes should be rejected"
    );
}

/// Test length boundary conditions.
#[test]
fn test_length_boundaries() {
    // 255 chars should pass
    let len_255 = "a".repeat(255);
    assert!(
        ServerConnectionString::new(&len_255).is_ok(),
        "255 characters should be allowed"
    );

    // 256 chars should pass (at limit)
    let len_256 = "a".repeat(256);
    assert!(
        ServerConnectionString::new(&len_256).is_ok(),
        "256 characters should be allowed"
    );

    // 257 chars should fail
    let len_257 = "a".repeat(257);
    assert!(
        ServerConnectionString::new(&len_257).is_err(),
        "257 characters should be rejected"
    );
}

/// Test that various control characters are rejected.
#[test]
fn test_all_control_chars_rejected() {
    // Test common control characters
    let controls = vec![
        ("\x00", "null"),
        ("\x01", "SOH"),
        ("\x07", "bell"),
        ("\x08", "backspace"),
        ("\x09", "tab"),
        ("\x0A", "LF"),
        ("\x0D", "CR"),
        ("\x1B", "escape"),
        ("\x7F", "delete"),
    ];

    for (control, name) in controls {
        let input = format!("server{control}");
        assert!(
            ServerConnectionString::new(&input).is_err(),
            "Control character {name} should be rejected"
        );
    }
}

/// Test that space is properly handled (allowed for trimming).
#[test]
fn test_space_handling() {
    // Leading/trailing spaces should be trimmed
    let with_spaces = "  server  ";
    let result = ServerConnectionString::new(with_spaces).unwrap();
    assert_eq!(result.as_str(), "server");

    // Space is the only control-like char allowed
    let only_spaces = "   ";
    assert!(
        ServerConnectionString::new(only_spaces).is_err(),
        "Only spaces should be rejected after trimming"
    );
}
