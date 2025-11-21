//! Blake3 checksum utilities for plugin integrity verification.
//!
//! Provides functions to calculate and verify Blake3 checksums for plugin
//! files. Checksums are stored in the format `"blake3:<hex>"` for easy
//! identification and future algorithm upgrades.

use crate::error::{PluginStoreError, Result};

/// Compares two strings in constant time to prevent timing attacks.
///
/// This function always compares all bytes regardless of whether a mismatch
/// is found early, preventing timing side-channel attacks where an attacker
/// could deduce the correct checksum byte-by-byte by measuring comparison time.
///
/// # Security
///
/// This implementation:
/// - Always processes the full length of both strings
/// - Uses bitwise OR to accumulate differences without short-circuiting
/// - Checks length equality in constant time
///
/// # Examples
///
/// ```
/// # use mcp_plugin_store::checksum::constant_time_compare;
/// // Identical strings
/// assert!(constant_time_compare("blake3:abc123", "blake3:abc123"));
///
/// // Different strings (same length)
/// assert!(!constant_time_compare("blake3:abc123", "blake3:def456"));
///
/// // Different lengths
/// assert!(!constant_time_compare("blake3:abc", "blake3:abcdef"));
/// ```
#[must_use]
#[allow(clippy::similar_names)]
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    // Check lengths first (this comparison is constant-time in Rust)
    let len_match = a.len() == b.len();

    // Get bytes for comparison
    let bytes_a = a.as_bytes();
    let bytes_b = b.as_bytes();

    // Always compare the maximum length to avoid timing leaks
    let max_len = a.len().max(b.len());

    // Accumulate differences using bitwise OR
    // This ensures all bytes are checked without short-circuiting
    let mut diff = 0u8;
    for i in 0..max_len {
        // Use get() with unwrap_or(0) to safely handle different lengths
        // The unwrap_or provides a neutral value that doesn't leak timing info
        let byte_a = bytes_a.get(i).copied().unwrap_or(0);
        let byte_b = bytes_b.get(i).copied().unwrap_or(0);

        // XOR reveals differences; OR accumulates them
        diff |= byte_a ^ byte_b;
    }

    // Both length and content must match
    len_match && diff == 0
}

/// Calculates Blake3 checksum for the given data.
///
/// Returns checksum in the format `"blake3:<hex>"` where `<hex>` is the
/// Blake3 hash in lowercase hexadecimal.
///
/// # Examples
///
/// ```
/// use mcp_plugin_store::checksum::calculate_checksum;
///
/// let data = b"Hello, world!";
/// let checksum = calculate_checksum(data);
///
/// assert!(checksum.starts_with("blake3:"));
/// assert_eq!(checksum.len(), 71); // "blake3:" + 64 hex chars
/// ```
///
/// # Performance
///
/// Blake3 is extremely fast - typically under 10ms for 1MB of data on
/// modern hardware. It's faster than MD5 while providing better security
/// properties.
#[must_use]
pub fn calculate_checksum(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    format!("blake3:{}", hash.to_hex())
}

/// Verifies that data matches the expected checksum.
///
/// Uses constant-time comparison to prevent timing side-channel attacks.
///
/// # Security
///
/// This function compares checksums in constant time to prevent attackers
/// from using timing measurements to determine the correct checksum.
/// While practical exploitability is low (requires local access and millions
/// of measurements), this is a defense-in-depth best practice.
///
/// # Errors
///
/// Returns [`PluginStoreError::ChecksumMismatch`] if the calculated checksum
/// doesn't match the expected value.
///
/// # Examples
///
/// ```
/// use mcp_plugin_store::checksum::{calculate_checksum, verify_checksum};
///
/// let data = b"Hello, world!";
/// let checksum = calculate_checksum(data);
///
/// // Verification succeeds with correct checksum
/// verify_checksum(data, &checksum, "test.txt").unwrap();
///
/// // Verification fails with wrong checksum
/// let result = verify_checksum(data, "blake3:wrong", "test.txt");
/// assert!(result.is_err());
/// ```
pub fn verify_checksum(data: &[u8], expected: &str, path: &str) -> Result<()> {
    let actual = calculate_checksum(data);

    // Use constant-time comparison to prevent timing attacks
    if !constant_time_compare(&actual, expected) {
        return Err(PluginStoreError::ChecksumMismatch {
            path: path.to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Validates checksum format without verifying content.
///
/// Checks that the checksum string follows the expected format:
/// - Starts with "blake3:"
/// - Followed by 64 lowercase hexadecimal characters
///
/// # Examples
///
/// ```
/// use mcp_plugin_store::checksum::is_valid_checksum_format;
///
/// // Valid format
/// assert!(is_valid_checksum_format(
///     "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
/// ));
///
/// // Invalid formats
/// assert!(!is_valid_checksum_format("md5:abc123"));
/// assert!(!is_valid_checksum_format("blake3:short"));
/// assert!(!is_valid_checksum_format("blake3:NOT_HEX!"));
/// ```
#[must_use]
pub fn is_valid_checksum_format(checksum: &str) -> bool {
    if !checksum.starts_with("blake3:") {
        return false;
    }

    let hex_part = &checksum[7..];
    if hex_part.len() != 64 {
        return false;
    }

    // Must be lowercase hexadecimal only
    hex_part
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_compare_identical() {
        // Identical strings should match
        assert!(constant_time_compare("blake3:abc123", "blake3:abc123"));
        assert!(constant_time_compare("", ""));
        assert!(constant_time_compare("test", "test"));
    }

    #[test]
    fn test_constant_time_compare_different_same_length() {
        // Different strings with same length should not match
        assert!(!constant_time_compare("blake3:abc123", "blake3:def456"));
        assert!(!constant_time_compare("test", "best"));

        // Differ at different positions
        assert!(!constant_time_compare("blake3:a00000", "blake3:b00000")); // First char
        assert!(!constant_time_compare("blake3:00000a", "blake3:00000b")); // Last char
    }

    #[test]
    fn test_constant_time_compare_different_lengths() {
        // Different lengths should not match
        assert!(!constant_time_compare("blake3:abc", "blake3:abcdef"));
        assert!(!constant_time_compare("long", "sh"));
        assert!(!constant_time_compare("", "nonempty"));
    }

    #[test]
    fn test_constant_time_compare_timing_consistency() {
        // Note: This is NOT a rigorous timing test and cannot prove constant-time behavior
        // Real timing analysis requires statistical methods and controlled environment
        // This test only ensures the implementation doesn't have obvious early-exit paths
        use std::time::Instant;

        let correct = "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let wrong_first = "blake3:x123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let wrong_last = "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdex";

        // Warmup to reduce impact of cold caches and CPU scaling
        for _ in 0..10 {
            let _ = constant_time_compare(correct, wrong_first);
            let _ = constant_time_compare(correct, wrong_last);
        }

        // Time comparisons (should be similar in duration)
        let start1 = Instant::now();
        for _ in 0..100 {
            let _ = constant_time_compare(correct, wrong_first);
        }
        let duration1 = start1.elapsed();

        let start2 = Instant::now();
        for _ in 0..100 {
            let _ = constant_time_compare(correct, wrong_last);
        }
        let duration2 = start2.elapsed();

        // Both should return false
        assert!(!constant_time_compare(correct, wrong_first));
        assert!(!constant_time_compare(correct, wrong_last));

        // Timing difference should be within reasonable bounds for CI environments
        // Very loose bound (10000x) to account for:
        // - CPU frequency scaling
        // - Context switching
        // - Cache effects
        // - System load on CI runners
        #[allow(clippy::cast_precision_loss)]
        let ratio = if duration1 < duration2 {
            duration2.as_nanos() as f64 / duration1.as_nanos().max(1) as f64
        } else {
            duration1.as_nanos() as f64 / duration2.as_nanos().max(1) as f64
        };

        // Sanity check: timing shouldn't be wildly different (10000x is very generous)
        // This catches obvious early-exit bugs but is NOT security validation
        assert!(
            ratio < 10000.0,
            "Timing ratio suspiciously large: {ratio} - may indicate early exit"
        );
    }

    #[test]
    fn test_calculate_checksum() {
        let data = b"Hello, world!";
        let checksum = calculate_checksum(data);

        assert!(checksum.starts_with("blake3:"));
        assert_eq!(checksum.len(), 71); // "blake3:" (7) + hex (64)

        // Same input should produce same output
        assert_eq!(checksum, calculate_checksum(data));

        // Different input should produce different output
        let other = calculate_checksum(b"Different data");
        assert_ne!(checksum, other);
    }

    #[test]
    fn test_verify_checksum_success() {
        let data = b"test data";
        let checksum = calculate_checksum(data);

        let result = verify_checksum(data, &checksum, "test.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_checksum_mismatch() {
        let data = b"test data";
        let wrong_checksum =
            "blake3:0000000000000000000000000000000000000000000000000000000000000000";

        let result = verify_checksum(data, wrong_checksum, "test.txt");
        assert!(result.is_err());

        match result {
            Err(PluginStoreError::ChecksumMismatch {
                path,
                expected,
                actual,
            }) => {
                assert_eq!(path, "test.txt");
                assert_eq!(expected, wrong_checksum);
                assert_ne!(actual, expected);
            }
            _ => panic!("Expected ChecksumMismatch error"),
        }
    }

    #[test]
    fn test_is_valid_checksum_format() {
        // Valid format
        assert!(is_valid_checksum_format(
            "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));

        // Invalid: wrong prefix
        assert!(!is_valid_checksum_format(
            "md5:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));

        // Invalid: too short
        assert!(!is_valid_checksum_format("blake3:abc123"));

        // Invalid: too long
        assert!(!is_valid_checksum_format(
            "blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef00"
        ));

        // Invalid: non-hex characters
        assert!(!is_valid_checksum_format(
            "blake3:gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg"
        ));

        // Invalid: uppercase (should be lowercase)
        assert!(!is_valid_checksum_format(
            "blake3:0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
        ));
    }

    #[test]
    fn test_checksum_deterministic() {
        let data = b"deterministic test";
        let checksum1 = calculate_checksum(data);
        let checksum2 = calculate_checksum(data);

        assert_eq!(checksum1, checksum2, "Checksum should be deterministic");
    }

    #[test]
    fn test_empty_data_checksum() {
        let empty = b"";
        let checksum = calculate_checksum(empty);

        assert!(checksum.starts_with("blake3:"));
        assert!(is_valid_checksum_format(&checksum));
    }

    #[test]
    fn test_large_data_checksum() {
        // Test with 1MB of data
        let large_data = vec![0u8; 1024 * 1024];
        let checksum = calculate_checksum(&large_data);

        assert!(checksum.starts_with("blake3:"));
        assert!(is_valid_checksum_format(&checksum));
    }
}
