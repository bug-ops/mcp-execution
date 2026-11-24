//! The Node.js preload that makes `mcp:<server-id>` and `mcp:helpers` importable.
//!
//! The preload is passed as a `--import data:` URL, so no file is written and no generation
//! change is needed. It registers synchronous module hooks (`module.registerHooks`, Node 22.15+):
//! `mcp:<id>` resolves to `<servers dir>/<id>/index.ts` and `mcp:helpers` is a virtual module with
//! `paginate` and `mapBounded`, each reporting truncation.
//!
//! When `MCP_EXECUTION_HELPER` is set the preload also claims the helper channel (the socket on
//! file descriptor 0, or a named pipe), sends the hello and immediately `unref`s the socket,
//! registers it under [`HELPER_CHANNEL_KEY`], replaces `process.stdin` with an empty stream and
//! deletes the helper variables from `process.env`, so children never learn the channel.

use std::path::Path;

use anyhow::{Context, Result};
use mcp_execution_core::helper_protocol::{
    HELPER_CHANNEL_KEY, HELPER_ENV, HELPER_PROTOCOL_VERSION, HELPER_TOKEN_ENV,
};

const PRELOAD_TEMPLATE: &str = include_str!("preload.js");
const HELPERS_SOURCE: &str = include_str!("helpers.js");

/// Builds the value of node's `--import` option for a run.
///
/// # Errors
///
/// Returns an error when `servers_dir` is not valid UTF-8, since it is embedded in the preload
/// source.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::import_hook::import_argument;
/// use std::path::Path;
///
/// let argument = import_argument(Path::new("/home/user/.claude/servers")).unwrap();
/// assert!(argument.starts_with("data:text/javascript,"));
/// ```
pub fn import_argument(servers_dir: &Path) -> Result<String> {
    let servers_dir = servers_dir
        .to_str()
        .context("the servers directory path is not valid UTF-8")?;
    // The caller-controlled path goes in last, so its text is never re-scanned for placeholders.
    let source = PRELOAD_TEMPLATE
        .replace("__HELPERS_SOURCE__", &json_string(HELPERS_SOURCE)?)
        .replace("__HELPER_ENV__", &json_string(HELPER_ENV)?)
        .replace("__HELPER_TOKEN_ENV__", &json_string(HELPER_TOKEN_ENV)?)
        .replace("__CHANNEL_KEY__", &json_string(HELPER_CHANNEL_KEY)?)
        .replace("__PROTOCOL_VERSION__", &HELPER_PROTOCOL_VERSION.to_string())
        .replace("__SERVERS_DIR__", &json_string(servers_dir)?);
    Ok(format!("data:text/javascript,{}", percent_encode(&source)))
}

fn json_string(text: &str) -> Result<String> {
    serde_json::to_string(text).context("cannot encode the preload source")
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            for nibble in [byte >> 4, byte & 0x0F] {
                encoded.extend(
                    char::from_digit(u32::from(nibble), 16).map(|c| c.to_ascii_uppercase()),
                );
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(percent_encode("a-b.c_d~1"), "a-b.c_d~1");
        assert_eq!(percent_encode("a b/é"), "a%20b%2F%C3%A9");
    }

    #[test]
    fn argument_embeds_the_servers_dir_and_helpers_as_json_strings() {
        let argument = import_argument(Path::new(r"C:\servers")).unwrap();
        let body = argument.strip_prefix("data:text/javascript,").unwrap();
        assert!(!body.contains(' ') && !body.contains('\n'));
        assert!(body.contains(&percent_encode(r#""C:\\servers""#)));
        assert!(
            !argument.contains("__"),
            "an unsubstituted placeholder remains"
        );
    }
}
