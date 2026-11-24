//! Bounds for the observed result shapes the runtime bridge may record.
//!
//! The bridge's opt-in recorder and the Rust reader of its output must apply identical limits.
//! Every limit is defined once here and rendered into the bridge through
//! [`BridgeContext`](crate::progressive::BridgeContext), so the TypeScript side copies no
//! number by hand.

/// Maximum nesting depth of a recorded shape.
pub const OBSERVED_SHAPE_MAX_DEPTH: usize = 4;

/// Maximum number of keys an object may have to be recorded key by key.
pub const OBSERVED_SHAPE_MAX_KEYS: usize = 16;

/// Maximum length in characters of a recorded object key.
pub const OBSERVED_SHAPE_MAX_KEY_LEN: usize = 32;

/// Maximum size in bytes of one recorded line.
pub const OBSERVED_SHAPE_MAX_LINE_BYTES: usize = 8192;
