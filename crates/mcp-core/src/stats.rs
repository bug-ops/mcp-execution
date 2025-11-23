//! Statistics collection and reporting for MCP Code Execution.
//!
//! This module provides strong-typed statistics infrastructure for tracking
//! performance, usage, and health metrics across all system components.
//!
//! # Architecture
//!
//! The statistics system follows a hierarchical structure:
//! - **`SystemStats`**: Top-level aggregation of all statistics
//! - **`BridgeStats`**: MCP Bridge caching and connection metrics
//! - **`RuntimeStats`**: WASM runtime execution metrics
//! - **`SkillStats`**: Skill storage and generation metrics
//!
//! All statistics types are:
//! - **Serializable**: Can be exported to JSON for monitoring
//! - **Thread-safe**: Implement `Send + Sync` for async use
//! - **Cloneable**: Support efficient snapshot capture
//!
//! # Design Principles
//!
//! Following Microsoft Rust Guidelines:
//! - Strong types over primitives
//! - Explicit snapshot timestamps using chrono
//! - Clear documentation with examples
//! - Zero-cost when not used (statistics are optional)
//!
//! # Examples
//!
//! ## Capturing System Statistics
//!
//! ```rust
//! use mcp_core::stats::{SystemStats, BridgeStats, RuntimeStats, SkillStats};
//!
//! // Create component statistics
//! let bridge = BridgeStats::new(100, 25, 5, 3, 2);
//! let runtime = RuntimeStats::new(50, 45, 2, 1, 15000);
//! let skills = SkillStats::new(10, 2048000, 5, 3);
//!
//! // Aggregate into system stats
//! let system = SystemStats::new(bridge, runtime, skills);
//!
//! // Export to JSON for monitoring
//! let json = serde_json::to_string_pretty(&system).unwrap();
//! println!("{}", json);
//! ```
//!
//! ## Using Stats Provider Trait
//!
//! ```rust
//! use mcp_core::stats::{StatsProvider, BridgeStats};
//!
//! struct MyBridge {
//!     total_calls: u32,
//!     cache_hits: u32,
//! }
//!
//! impl StatsProvider for MyBridge {
//!     type Stats = BridgeStats;
//!
//!     fn capture_stats(&self) -> Self::Stats {
//!         BridgeStats::new(
//!             self.total_calls,
//!             self.cache_hits,
//!             0, // active_connections
//!             0, // total_connections
//!             0, // connection_failures
//!         )
//!     }
//! }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Top-level system statistics aggregating metrics from all components.
///
/// Provides a complete snapshot of system health and performance at a
/// specific point in time. All component statistics are captured with
/// synchronized timestamps.
///
/// # Thread Safety
///
/// Implements `Send + Sync` for safe sharing across async tasks.
///
/// # Serialization
///
/// Serializes to JSON with human-readable timestamps:
///
/// ```json
/// {
///   "snapshot_time": "2025-01-15T10:30:00Z",
///   "bridge": { ... },
///   "runtime": { ... },
///   "skills": { ... }
/// }
/// ```
///
/// # Examples
///
/// ```rust
/// use mcp_core::stats::{SystemStats, BridgeStats, RuntimeStats, SkillStats};
///
/// let system = SystemStats::new(
///     BridgeStats::default(),
///     RuntimeStats::default(),
///     SkillStats::default(),
/// );
///
/// assert!(system.snapshot_time() <= chrono::Utc::now());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    /// Timestamp when this snapshot was captured (UTC).
    snapshot_time: DateTime<Utc>,

    /// MCP Bridge statistics (caching, connections).
    bridge: BridgeStats,

    /// WASM Runtime statistics (execution, performance).
    runtime: RuntimeStats,

    /// Skill Storage statistics (disk usage, generation).
    skills: SkillStats,
}

impl SystemStats {
    /// Creates a new system statistics snapshot with current timestamp.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::{SystemStats, BridgeStats, RuntimeStats, SkillStats};
    ///
    /// let stats = SystemStats::new(
    ///     BridgeStats::default(),
    ///     RuntimeStats::default(),
    ///     SkillStats::default(),
    /// );
    /// ```
    #[must_use]
    pub fn new(bridge: BridgeStats, runtime: RuntimeStats, skills: SkillStats) -> Self {
        Self {
            snapshot_time: Utc::now(),
            bridge,
            runtime,
            skills,
        }
    }

    /// Returns the timestamp when this snapshot was captured.
    #[must_use]
    pub const fn snapshot_time(&self) -> DateTime<Utc> {
        self.snapshot_time
    }

    /// Returns a reference to the bridge statistics.
    #[must_use]
    pub const fn bridge(&self) -> &BridgeStats {
        &self.bridge
    }

    /// Returns a reference to the runtime statistics.
    #[must_use]
    pub const fn runtime(&self) -> &RuntimeStats {
        &self.runtime
    }

    /// Returns a reference to the skill statistics.
    #[must_use]
    pub const fn skills(&self) -> &SkillStats {
        &self.skills
    }

    /// Calculates overall cache hit rate across bridge and runtime.
    ///
    /// Returns `None` if there were no cacheable operations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::{SystemStats, BridgeStats, RuntimeStats, SkillStats};
    ///
    /// let bridge = BridgeStats::new(100, 75, 0, 0, 0); // 75% hit rate
    /// let runtime = RuntimeStats::new(50, 40, 0, 0, 0); // 80% hit rate
    /// let stats = SystemStats::new(bridge, runtime, SkillStats::default());
    ///
    /// let hit_rate = stats.overall_cache_hit_rate().unwrap();
    /// assert!((hit_rate - 0.7666).abs() < 0.001); // (75+40)/(100+50) ≈ 76.66%
    /// ```
    #[must_use]
    pub fn overall_cache_hit_rate(&self) -> Option<f64> {
        let total_bridge = self.bridge.total_tool_calls;
        let total_runtime = self.runtime.total_executions;
        let hits_bridge = self.bridge.cache_hits;
        let hits_runtime = self.runtime.cache_hits;

        let total = total_bridge + total_runtime;
        if total == 0 {
            return None;
        }

        let hits = hits_bridge + hits_runtime;
        Some(f64::from(hits) / f64::from(total))
    }
}

/// MCP Bridge statistics tracking tool call caching and server connections.
///
/// Monitors the performance and reliability of the bridge component that
/// proxies calls between WASM modules and real MCP servers.
///
/// # Metrics
///
/// - **Tool Calls**: Total invocations and cache performance
/// - **Connections**: Active connections and failure tracking
/// - **Cache Hit Rate**: Percentage of calls served from cache
///
/// # Examples
///
/// ```rust
/// use mcp_core::stats::BridgeStats;
///
/// let stats = BridgeStats::new(
///     1000,  // total_tool_calls
///     850,   // cache_hits (85% hit rate)
///     5,     // active_connections
///     120,   // total_connections
///     3,     // connection_failures (97.5% success rate)
/// );
///
/// assert_eq!(stats.cache_hit_rate(), Some(0.85));
/// assert_eq!(stats.connection_success_rate(), Some(0.975));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStats {
    /// Total tool calls made through the bridge.
    pub total_tool_calls: u32,

    /// Number of tool calls served from cache.
    pub cache_hits: u32,

    /// Currently active server connections.
    pub active_connections: u32,

    /// Total connections established (lifetime).
    pub total_connections: u32,

    /// Number of failed connection attempts.
    pub connection_failures: u32,
}

impl BridgeStats {
    /// Creates new bridge statistics with specified values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::BridgeStats;
    ///
    /// let stats = BridgeStats::new(100, 75, 3, 10, 1);
    /// assert_eq!(stats.total_tool_calls, 100);
    /// ```
    #[must_use]
    pub const fn new(
        total_tool_calls: u32,
        cache_hits: u32,
        active_connections: u32,
        total_connections: u32,
        connection_failures: u32,
    ) -> Self {
        Self {
            total_tool_calls,
            cache_hits,
            active_connections,
            total_connections,
            connection_failures,
        }
    }

    /// Calculates cache hit rate as a fraction (0.0 to 1.0).
    ///
    /// Returns `None` if no tool calls have been made.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::BridgeStats;
    ///
    /// let stats = BridgeStats::new(100, 85, 0, 0, 0);
    /// assert_eq!(stats.cache_hit_rate(), Some(0.85));
    ///
    /// let empty = BridgeStats::default();
    /// assert_eq!(empty.cache_hit_rate(), None);
    /// ```
    #[must_use]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.total_tool_calls == 0 {
            return None;
        }
        Some(f64::from(self.cache_hits) / f64::from(self.total_tool_calls))
    }

    /// Calculates connection success rate as a fraction (0.0 to 1.0).
    ///
    /// Returns `None` if no connection attempts have been made.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::BridgeStats;
    ///
    /// let stats = BridgeStats::new(0, 0, 0, 100, 5);
    /// assert_eq!(stats.connection_success_rate(), Some(0.95));
    /// ```
    #[must_use]
    pub fn connection_success_rate(&self) -> Option<f64> {
        if self.total_connections == 0 {
            return None;
        }
        let successes = self.total_connections - self.connection_failures;
        Some(f64::from(successes) / f64::from(self.total_connections))
    }
}

impl Default for BridgeStats {
    fn default() -> Self {
        Self::new(0, 0, 0, 0, 0)
    }
}

/// WASM Runtime statistics tracking module execution and performance.
///
/// Monitors the performance, reliability, and resource usage of WASM
/// module execution in the sandboxed runtime environment.
///
/// # Metrics
///
/// - **Executions**: Total runs, cache hits, and failures
/// - **Performance**: Average execution time in microseconds
/// - **Compilation**: Module cache effectiveness
///
/// # Examples
///
/// ```rust
/// use mcp_core::stats::RuntimeStats;
///
/// let stats = RuntimeStats::new(
///     500,    // total_executions
///     450,    // cache_hits (90% hit rate)
///     10,     // execution_failures (98% success rate)
///     5,      // compilation_failures
///     150000, // avg_execution_time_us (150ms average)
/// );
///
/// assert_eq!(stats.cache_hit_rate(), Some(0.9));
/// assert_eq!(stats.execution_success_rate(), Some(0.98));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStats {
    /// Total WASM module executions attempted.
    pub total_executions: u32,

    /// Number of executions using cached compiled modules.
    pub cache_hits: u32,

    /// Number of failed executions (panics, timeouts, OOM).
    pub execution_failures: u32,

    /// Number of failed module compilations.
    pub compilation_failures: u32,

    /// Average execution time in microseconds.
    pub avg_execution_time_us: u64,
}

impl RuntimeStats {
    /// Creates new runtime statistics with specified values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::RuntimeStats;
    ///
    /// let stats = RuntimeStats::new(100, 90, 2, 1, 50000);
    /// assert_eq!(stats.total_executions, 100);
    /// ```
    #[must_use]
    pub const fn new(
        total_executions: u32,
        cache_hits: u32,
        execution_failures: u32,
        compilation_failures: u32,
        avg_execution_time_us: u64,
    ) -> Self {
        Self {
            total_executions,
            cache_hits,
            execution_failures,
            compilation_failures,
            avg_execution_time_us,
        }
    }

    /// Calculates module cache hit rate as a fraction (0.0 to 1.0).
    ///
    /// Returns `None` if no executions have been attempted.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::RuntimeStats;
    ///
    /// let stats = RuntimeStats::new(100, 95, 0, 0, 0);
    /// assert_eq!(stats.cache_hit_rate(), Some(0.95));
    /// ```
    #[must_use]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.total_executions == 0 {
            return None;
        }
        Some(f64::from(self.cache_hits) / f64::from(self.total_executions))
    }

    /// Calculates execution success rate as a fraction (0.0 to 1.0).
    ///
    /// Returns `None` if no executions have been attempted.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::RuntimeStats;
    ///
    /// let stats = RuntimeStats::new(100, 0, 5, 0, 0);
    /// assert_eq!(stats.execution_success_rate(), Some(0.95));
    /// ```
    #[must_use]
    pub fn execution_success_rate(&self) -> Option<f64> {
        if self.total_executions == 0 {
            return None;
        }
        let successes = self.total_executions - self.execution_failures;
        Some(f64::from(successes) / f64::from(self.total_executions))
    }

    /// Returns average execution time as a Duration.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::RuntimeStats;
    /// use std::time::Duration;
    ///
    /// let stats = RuntimeStats::new(0, 0, 0, 0, 150_000); // 150ms
    /// assert_eq!(stats.avg_execution_time(), Duration::from_millis(150));
    /// ```
    #[must_use]
    pub const fn avg_execution_time(&self) -> Duration {
        Duration::from_micros(self.avg_execution_time_us)
    }
}

impl Default for RuntimeStats {
    fn default() -> Self {
        Self::new(0, 0, 0, 0, 0)
    }
}

/// Skill Storage statistics tracking disk usage and skill generation.
///
/// Monitors storage utilization and generation activity for skills
/// stored in the `.claude/skills/` directory structure.
///
/// # Metrics
///
/// - **Storage**: Number of skills and total disk space used
/// - **Generation**: Successful and failed skill generation attempts
///
/// # Examples
///
/// ```rust
/// use mcp_core::stats::SkillStats;
///
/// let stats = SkillStats::new(
///     25,         // total_skills
///     52428800,   // total_storage_bytes (50 MB)
///     100,        // generation_successes
///     3,          // generation_failures (97% success rate)
/// );
///
/// // Use approximate comparison for floating point
/// let rate = stats.generation_success_rate().unwrap();
/// assert!((rate - 0.9708).abs() < 0.001);
/// assert_eq!(stats.avg_skill_size_bytes(), Some(2097152)); // 2 MB per skill
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStats {
    /// Total number of skills stored.
    pub total_skills: u32,

    /// Total disk space used by all skills (bytes).
    pub total_storage_bytes: u64,

    /// Number of successful skill generations.
    pub generation_successes: u32,

    /// Number of failed skill generation attempts.
    pub generation_failures: u32,
}

impl SkillStats {
    /// Creates new skill storage statistics with specified values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::SkillStats;
    ///
    /// let stats = SkillStats::new(10, 10_485_760, 50, 2);
    /// assert_eq!(stats.total_skills, 10);
    /// ```
    #[must_use]
    pub const fn new(
        total_skills: u32,
        total_storage_bytes: u64,
        generation_successes: u32,
        generation_failures: u32,
    ) -> Self {
        Self {
            total_skills,
            total_storage_bytes,
            generation_successes,
            generation_failures,
        }
    }

    /// Calculates skill generation success rate as a fraction (0.0 to 1.0).
    ///
    /// Returns `None` if no generation attempts have been made.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::SkillStats;
    ///
    /// let stats = SkillStats::new(0, 0, 95, 5);
    /// assert_eq!(stats.generation_success_rate(), Some(0.95));
    /// ```
    #[must_use]
    pub fn generation_success_rate(&self) -> Option<f64> {
        let total = self.generation_successes + self.generation_failures;
        if total == 0 {
            return None;
        }
        Some(f64::from(self.generation_successes) / f64::from(total))
    }

    /// Calculates average skill size in bytes.
    ///
    /// Returns `None` if no skills are stored.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use mcp_core::stats::SkillStats;
    ///
    /// let stats = SkillStats::new(5, 10_485_760, 0, 0); // 10 MB / 5 skills
    /// assert_eq!(stats.avg_skill_size_bytes(), Some(2_097_152)); // 2 MB each
    /// ```
    #[must_use]
    pub fn avg_skill_size_bytes(&self) -> Option<u64> {
        if self.total_skills == 0 {
            return None;
        }
        Some(self.total_storage_bytes / u64::from(self.total_skills))
    }
}

impl Default for SkillStats {
    fn default() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

/// Trait for components that can provide statistics snapshots.
///
/// Implement this trait to enable consistent statistics collection
/// across different system components.
///
/// # Type Safety
///
/// Each implementor defines its own `Stats` associated type, ensuring
/// type-safe statistics capture.
///
/// # Examples
///
/// ```rust
/// use mcp_core::stats::{StatsProvider, RuntimeStats};
///
/// struct MockRuntime {
///     executions: u32,
/// }
///
/// impl StatsProvider for MockRuntime {
///     type Stats = RuntimeStats;
///
///     fn capture_stats(&self) -> Self::Stats {
///         RuntimeStats::new(self.executions, 0, 0, 0, 0)
///     }
/// }
///
/// let runtime = MockRuntime { executions: 42 };
/// let stats = runtime.capture_stats();
/// assert_eq!(stats.total_executions, 42);
/// ```
pub trait StatsProvider {
    /// The statistics type produced by this provider.
    type Stats: Clone + std::fmt::Debug + Serialize;

    /// Captures a snapshot of current statistics.
    ///
    /// This method should be efficient and non-blocking, suitable for
    /// frequent polling by monitoring systems.
    fn capture_stats(&self) -> Self::Stats;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_stats_creation() {
        let bridge = BridgeStats::new(100, 80, 5, 10, 2);
        let runtime = RuntimeStats::new(50, 45, 1, 0, 100_000);
        let skills = SkillStats::new(10, 10_000_000, 20, 1);

        let system = SystemStats::new(bridge, runtime, skills);

        assert!(system.snapshot_time() <= Utc::now());
        assert_eq!(system.bridge().total_tool_calls, 100);
        assert_eq!(system.runtime().total_executions, 50);
        assert_eq!(system.skills().total_skills, 10);
    }

    #[test]
    fn test_system_stats_overall_cache_hit_rate() {
        let bridge = BridgeStats::new(100, 75, 0, 0, 0); // 75% hit rate
        let runtime = RuntimeStats::new(50, 40, 0, 0, 0); // 80% hit rate
        let skills = SkillStats::default();

        let system = SystemStats::new(bridge, runtime, skills);

        // (75 + 40) / (100 + 50) = 115 / 150 = 0.7666...
        let hit_rate = system.overall_cache_hit_rate().unwrap();
        assert!((hit_rate - 0.7666).abs() < 0.001);
    }

    #[test]
    fn test_system_stats_overall_cache_hit_rate_no_operations() {
        let system = SystemStats::new(
            BridgeStats::default(),
            RuntimeStats::default(),
            SkillStats::default(),
        );

        assert_eq!(system.overall_cache_hit_rate(), None);
    }

    #[test]
    fn test_bridge_stats_cache_hit_rate() {
        let stats = BridgeStats::new(100, 85, 0, 0, 0);
        assert_eq!(stats.cache_hit_rate(), Some(0.85));

        let empty = BridgeStats::default();
        assert_eq!(empty.cache_hit_rate(), None);
    }

    #[test]
    fn test_bridge_stats_connection_success_rate() {
        let stats = BridgeStats::new(0, 0, 0, 100, 5);
        assert_eq!(stats.connection_success_rate(), Some(0.95));

        let no_connections = BridgeStats::new(100, 80, 0, 0, 0);
        assert_eq!(no_connections.connection_success_rate(), None);
    }

    #[test]
    fn test_runtime_stats_cache_hit_rate() {
        let stats = RuntimeStats::new(100, 90, 0, 0, 0);
        assert_eq!(stats.cache_hit_rate(), Some(0.9));

        let empty = RuntimeStats::default();
        assert_eq!(empty.cache_hit_rate(), None);
    }

    #[test]
    fn test_runtime_stats_execution_success_rate() {
        let stats = RuntimeStats::new(100, 0, 5, 0, 0);
        assert_eq!(stats.execution_success_rate(), Some(0.95));

        let perfect = RuntimeStats::new(100, 50, 0, 0, 0);
        assert_eq!(perfect.execution_success_rate(), Some(1.0));
    }

    #[test]
    fn test_runtime_stats_avg_execution_time() {
        let stats = RuntimeStats::new(0, 0, 0, 0, 150_000);
        assert_eq!(stats.avg_execution_time(), Duration::from_millis(150));
    }

    #[test]
    fn test_skill_stats_generation_success_rate() {
        let stats = SkillStats::new(0, 0, 95, 5);
        assert_eq!(stats.generation_success_rate(), Some(0.95));

        let empty = SkillStats::default();
        assert_eq!(empty.generation_success_rate(), None);
    }

    #[test]
    fn test_skill_stats_avg_skill_size() {
        let stats = SkillStats::new(5, 10_485_760, 0, 0);
        assert_eq!(stats.avg_skill_size_bytes(), Some(2_097_152));

        let no_skills = SkillStats::new(0, 0, 0, 0);
        assert_eq!(no_skills.avg_skill_size_bytes(), None);
    }

    #[test]
    fn test_stats_serialization() {
        let bridge = BridgeStats::new(100, 80, 5, 10, 2);
        let json = serde_json::to_string(&bridge).unwrap();
        let deserialized: BridgeStats = serde_json::from_str(&json).unwrap();
        assert_eq!(bridge.total_tool_calls, deserialized.total_tool_calls);
    }

    #[test]
    fn test_system_stats_serialization() {
        let system = SystemStats::new(
            BridgeStats::new(100, 80, 5, 10, 2),
            RuntimeStats::new(50, 45, 1, 0, 100_000),
            SkillStats::new(10, 10_000_000, 20, 1),
        );

        let json = serde_json::to_string(&system).unwrap();
        let deserialized: SystemStats = serde_json::from_str(&json).unwrap();

        assert_eq!(
            system.bridge().total_tool_calls,
            deserialized.bridge().total_tool_calls
        );
        assert_eq!(
            system.runtime().total_executions,
            deserialized.runtime().total_executions
        );
        assert_eq!(
            system.skills().total_skills,
            deserialized.skills().total_skills
        );
    }

    #[test]
    fn test_stats_provider_trait() {
        struct MockRuntime {
            executions: u32,
        }

        impl StatsProvider for MockRuntime {
            type Stats = RuntimeStats;

            fn capture_stats(&self) -> Self::Stats {
                RuntimeStats::new(self.executions, 0, 0, 0, 0)
            }
        }

        let runtime = MockRuntime { executions: 42 };
        let stats = runtime.capture_stats();
        assert_eq!(stats.total_executions, 42);
    }

    #[test]
    fn test_default_values() {
        let bridge = BridgeStats::default();
        assert_eq!(bridge.total_tool_calls, 0);
        assert_eq!(bridge.cache_hits, 0);

        let runtime = RuntimeStats::default();
        assert_eq!(runtime.total_executions, 0);

        let skills = SkillStats::default();
        assert_eq!(skills.total_skills, 0);
    }
}
