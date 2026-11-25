//! Shared `tracing` filter helpers for the workspace binaries.
//!
//! Both `mcp-execution-cli` and `mcp-execution-server` build their log filter through
//! [`cap_rmcp_log_level`], so the `rmcp` debug-logging cap exists in exactly one place.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// Caps `rmcp`'s own `tracing` targets at `cap`, on top of whatever base filter is already in
/// effect.
///
/// `rmcp`'s transport layer logs raw, unsanitized peer input at `debug` level (e.g.
/// `rmcp::transport::async_rw`'s parse-failure line embeds the offending line verbatim). Both
/// binaries talk to untrusted MCP peers, so without this cap a broad `debug` filter (`--verbose`,
/// or `RUST_LOG=debug`) streams that text into the log unsanitized (issue #421). This closes the
/// *debug-level, raw-line* logging specifically -- `rmcp` also logs a `Debug`-formatted peer
/// notification at `info`, which a cap of `info` does not and cannot suppress; that site is
/// mitigated by `Debug`-escaping control characters, not eliminated.
///
/// Applied via [`EnvFilter::add_directive`] to the filter already selected by the caller
/// (including one parsed from `RUST_LOG`), not folded into only a fallback string: a directive
/// added solely to a fallback is dead code whenever `RUST_LOG` parses successfully.
///
/// Directive sets order by target specificity, so this `rmcp=<cap>` directive (more specific than
/// a bare global `debug`) wins over it. An operator who explicitly sets a *more specific*
/// directive, e.g. `RUST_LOG=rmcp::transport=debug`, still wins over this one -- that is
/// intentional: this cap closes the accidental broad-`debug` case, not an operator's deliberate
/// request for `rmcp` transport debug logs -- **this is the escape hatch**: a target under
/// `rmcp::` (not the bare `rmcp` target this cap sets) survives the cap and can be raised back to
/// `debug` explicitly. Note this is target *specificity*, not level: an equally-specific
/// `RUST_LOG=rmcp=debug` (same target as this cap, different level) is *replaced* by this cap's
/// `rmcp=<cap>`, not merged with it -- `tracing_subscriber`'s `Directive` ordering does not
/// compare level, so a same-target `add_directive` call overwrites the existing entry. Both
/// behaviors are pinned by tests rather than assumed.
///
/// # Panics
///
/// Never in practice: the `rmcp=<level>` directive string is statically valid for every
/// [`LevelFilter`].
///
/// # Examples
///
/// ```
/// use mcp_execution_core::log_filter::cap_rmcp_log_level;
/// use tracing_subscriber::EnvFilter;
/// use tracing_subscriber::filter::LevelFilter;
///
/// let filter = cap_rmcp_log_level(EnvFilter::new("debug"), LevelFilter::INFO);
/// assert!(filter.to_string().contains("rmcp=info"));
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "tracing-filter")))]
#[expect(clippy::expect_used, reason = "static directive string is valid")]
#[must_use]
pub fn cap_rmcp_log_level(filter: EnvFilter, cap: LevelFilter) -> EnvFilter {
    filter.add_directive(
        format!("rmcp={cap}")
            .parse()
            .expect("\"rmcp=<level>\" is always a valid directive"),
    )
}

#[cfg(test)]
mod tests {
    use super::{EnvFilter, LevelFilter, cap_rmcp_log_level};
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt as _;

    #[derive(Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.lock().unwrap().flush()
        }
    }

    /// Builds `cap_rmcp_log_level(EnvFilter::new(base_filter), INFO)`, wires it into a real
    /// `fmt::layer()` over a scoped subscriber, emits one `rmcp`-targeted `debug!` (standing in
    /// for `rmcp`'s raw-peer-line logging) and one own-target `debug!`, and returns
    /// `(rmcp_line_visible, own_line_visible)`. Does not touch `RUST_LOG`.
    fn rmcp_capped_filter_captures(base_filter: &str) -> (bool, bool) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let make_writer = {
            let buf = buf.clone();
            move || SharedBuf(buf.clone())
        };

        let filter = cap_rmcp_log_level(EnvFilter::new(base_filter), LevelFilter::INFO);
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .with_writer(make_writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(
                target: "rmcp::transport::async_rw",
                "raw untrusted peer line"
            );
            tracing::debug!(target: "mcp_execution_cli", "own debug event");
        });

        let written = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        (
            written.contains("raw untrusted peer line"),
            written.contains("own debug event"),
        )
    }

    /// Regression coverage for issue #421: a broad `debug` base filter must not let `rmcp`'s own
    /// targets through, while the caller's own `debug` events must still pass.
    #[test]
    fn cap_rmcp_log_level_suppresses_rmcp_debug_but_keeps_own_debug() {
        let (rmcp_visible, own_visible) = rmcp_capped_filter_captures("debug");
        assert!(
            !rmcp_visible,
            "rmcp debug line was not suppressed under a global `debug` base"
        );
        assert!(
            own_visible,
            "own debug event was unexpectedly suppressed under a global `debug` base"
        );
    }

    /// `tracing_subscriber`'s `Directive` ordering does not compare level, so
    /// `EnvFilter::add_directive` *replaces* a same-target directive rather than merging it: an
    /// explicit `RUST_LOG=rmcp=debug` is downgraded to the cap. The escape hatch is a *more
    /// specific* target (see the test below).
    #[test]
    fn cap_rmcp_log_level_replaces_a_same_target_rmcp_debug_directive() {
        let (rmcp_visible, _) = rmcp_capped_filter_captures("rmcp=debug");
        assert!(
            !rmcp_visible,
            "RUST_LOG=rmcp=debug was expected to be replaced by this cap's rmcp=info, not merged \
             with it -- if this now fails, `tracing_subscriber`'s directive-merge behavior \
             changed and `cap_rmcp_log_level`'s doc comment needs updating"
        );
    }

    /// A target *more specific* than `rmcp` (e.g. `RUST_LOG=rmcp::transport=debug`) is not
    /// overwritten by the cap: the documented escape hatch for deliberate rmcp transport debug.
    #[test]
    fn cap_rmcp_log_level_does_not_override_a_more_specific_rmcp_target() {
        let (rmcp_visible, _) = rmcp_capped_filter_captures("rmcp::transport=debug");
        assert!(
            rmcp_visible,
            "RUST_LOG=rmcp::transport=debug should still surface rmcp debug output -- a more \
             specific target must win over this cap's rmcp=info"
        );
    }

    /// The cap level is honoured, not hard-coded to `info`.
    #[test]
    fn cap_rmcp_log_level_honours_the_given_level() {
        let filter = cap_rmcp_log_level(EnvFilter::new("debug"), LevelFilter::WARN);
        assert!(filter.to_string().contains("rmcp=warn"));
    }
}
