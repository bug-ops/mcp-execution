//! Performance metrics collection and reporting.
//!
//! Tracks key metrics for the MCP Code Execution workflow including:
//! - Execution time for each phase
//! - Token usage and savings
//! - Cache performance
//! - Resource utilization

use std::time::Instant;

/// Comprehensive metrics for end-to-end workflow execution.
///
/// Tracks timing, token usage, and performance characteristics across
/// all phases of the MCP Code Execution pipeline.
///
/// # Examples
///
/// ```
/// use mcp_examples::metrics::Metrics;
///
/// let mut metrics = Metrics::new();
/// metrics.start_introspection();
/// // ... perform introspection
/// metrics.end_introspection();
///
/// println!("Introspection took: {}ms", metrics.introspection_time_ms);
/// ```
#[derive(Debug, Clone)]
pub struct Metrics {
    /// Time spent introspecting the MCP server (in milliseconds).
    pub introspection_time_ms: u64,

    /// Time spent generating code (in milliseconds).
    pub code_generation_time_ms: u64,

    /// Time spent loading code into VFS (in milliseconds).
    pub vfs_load_time_ms: u64,

    /// Time spent compiling WASM module (in milliseconds).
    pub wasm_compilation_time_ms: u64,

    /// Time spent executing the WASM module (in milliseconds).
    pub execution_time_ms: u64,

    /// Total end-to-end time (in milliseconds).
    pub total_time_ms: u64,

    /// Whether the execution used cached data.
    pub cache_hit: bool,

    /// Token savings percentage compared to standard MCP.
    pub token_savings_percent: f64,

    /// Number of tools discovered.
    pub tools_discovered: usize,

    /// Number of files generated.
    pub files_generated: usize,

    /// Size of generated code in bytes.
    pub generated_code_bytes: usize,

    /// Number of MCP calls made.
    pub mcp_calls: usize,

    // Internal timing fields
    introspection_start: Option<Instant>,
    codegen_start: Option<Instant>,
    vfs_start: Option<Instant>,
    wasm_compile_start: Option<Instant>,
    execution_start: Option<Instant>,
    total_start: Option<Instant>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    /// Creates a new metrics tracker.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let metrics = Metrics::new();
    /// assert_eq!(metrics.total_time_ms, 0);
    /// ```
    pub fn new() -> Self {
        Self {
            introspection_time_ms: 0,
            code_generation_time_ms: 0,
            vfs_load_time_ms: 0,
            wasm_compilation_time_ms: 0,
            execution_time_ms: 0,
            total_time_ms: 0,
            cache_hit: false,
            token_savings_percent: 0.0,
            tools_discovered: 0,
            files_generated: 0,
            generated_code_bytes: 0,
            mcp_calls: 0,
            introspection_start: None,
            codegen_start: None,
            vfs_start: None,
            wasm_compile_start: None,
            execution_start: None,
            total_start: None,
        }
    }

    /// Starts timing the total workflow.
    pub fn start_total(&mut self) {
        self.total_start = Some(Instant::now());
    }

    /// Ends timing the total workflow.
    pub fn end_total(&mut self) {
        if let Some(start) = self.total_start {
            self.total_time_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Starts timing the introspection phase.
    pub fn start_introspection(&mut self) {
        self.introspection_start = Some(Instant::now());
    }

    /// Ends timing the introspection phase.
    pub fn end_introspection(&mut self) {
        if let Some(start) = self.introspection_start {
            self.introspection_time_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Starts timing the code generation phase.
    pub fn start_code_generation(&mut self) {
        self.codegen_start = Some(Instant::now());
    }

    /// Ends timing the code generation phase.
    pub fn end_code_generation(&mut self) {
        if let Some(start) = self.codegen_start {
            self.code_generation_time_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Starts timing the VFS loading phase.
    pub fn start_vfs_load(&mut self) {
        self.vfs_start = Some(Instant::now());
    }

    /// Ends timing the VFS loading phase.
    pub fn end_vfs_load(&mut self) {
        if let Some(start) = self.vfs_start {
            self.vfs_load_time_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Starts timing the WASM compilation phase.
    pub fn start_wasm_compilation(&mut self) {
        self.wasm_compile_start = Some(Instant::now());
    }

    /// Ends timing the WASM compilation phase.
    pub fn end_wasm_compilation(&mut self) {
        if let Some(start) = self.wasm_compile_start {
            self.wasm_compilation_time_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Starts timing the execution phase.
    pub fn start_execution(&mut self) {
        self.execution_start = Some(Instant::now());
    }

    /// Ends timing the execution phase.
    pub fn end_execution(&mut self) {
        if let Some(start) = self.execution_start {
            self.execution_time_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Calculates total overhead (excluding execution).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let mut metrics = Metrics::new();
    /// metrics.introspection_time_ms = 100;
    /// metrics.code_generation_time_ms = 50;
    /// metrics.vfs_load_time_ms = 25;
    /// metrics.wasm_compilation_time_ms = 75;
    ///
    /// assert_eq!(metrics.total_overhead_ms(), 250);
    /// ```
    pub fn total_overhead_ms(&self) -> u64 {
        self.introspection_time_ms
            + self.code_generation_time_ms
            + self.vfs_load_time_ms
            + self.wasm_compilation_time_ms
    }

    /// Checks if execution overhead meets the target (<50ms).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let mut metrics = Metrics::new();
    /// metrics.execution_time_ms = 30;
    /// assert!(metrics.meets_execution_target());
    ///
    /// metrics.execution_time_ms = 60;
    /// assert!(!metrics.meets_execution_target());
    /// ```
    pub fn meets_execution_target(&self) -> bool {
        self.execution_time_ms < 50
    }

    /// Checks if WASM compilation meets the target (<100ms).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let mut metrics = Metrics::new();
    /// metrics.wasm_compilation_time_ms = 80;
    /// assert!(metrics.meets_compilation_target());
    ///
    /// metrics.wasm_compilation_time_ms = 120;
    /// assert!(!metrics.meets_compilation_target());
    /// ```
    pub fn meets_compilation_target(&self) -> bool {
        self.wasm_compilation_time_ms < 100
    }

    /// Checks if token savings meet the target (≥90%).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let mut metrics = Metrics::new();
    /// metrics.token_savings_percent = 92.5;
    /// assert!(metrics.meets_token_target());
    ///
    /// metrics.token_savings_percent = 85.0;
    /// assert!(!metrics.meets_token_target());
    /// ```
    pub fn meets_token_target(&self) -> bool {
        self.token_savings_percent >= 90.0
    }

    /// Checks if all performance targets are met.
    ///
    /// Returns `true` only if:
    /// - Execution overhead < 50ms
    /// - WASM compilation < 100ms
    /// - Token savings ≥ 90%
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let mut metrics = Metrics::new();
    /// metrics.execution_time_ms = 30;
    /// metrics.wasm_compilation_time_ms = 80;
    /// metrics.token_savings_percent = 92.0;
    ///
    /// assert!(metrics.meets_all_targets());
    /// ```
    pub fn meets_all_targets(&self) -> bool {
        self.meets_execution_target()
            && self.meets_compilation_target()
            && self.meets_token_target()
    }

    /// Formats metrics as a human-readable report.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::metrics::Metrics;
    ///
    /// let mut metrics = Metrics::new();
    /// metrics.total_time_ms = 250;
    /// metrics.token_savings_percent = 92.5;
    ///
    /// let report = metrics.format_report();
    /// assert!(report.contains("Total Time"));
    /// assert!(report.contains("Token Savings"));
    /// ```
    pub fn format_report(&self) -> String {
        let mut report = String::new();

        report.push_str("═══════════════════════════════════════════════\n");
        report.push_str("         PERFORMANCE METRICS REPORT           \n");
        report.push_str("═══════════════════════════════════════════════\n\n");

        report.push_str("Timing Breakdown:\n");
        report.push_str(&format!(
            "  Introspection:      {:>6} ms\n",
            self.introspection_time_ms
        ));
        report.push_str(&format!(
            "  Code Generation:    {:>6} ms\n",
            self.code_generation_time_ms
        ));
        report.push_str(&format!(
            "  VFS Load:           {:>6} ms\n",
            self.vfs_load_time_ms
        ));
        report.push_str(&format!(
            "  WASM Compilation:   {:>6} ms {}\n",
            self.wasm_compilation_time_ms,
            if self.meets_compilation_target() {
                "✓"
            } else {
                "✗"
            }
        ));
        report.push_str(&format!(
            "  Execution:          {:>6} ms {}\n",
            self.execution_time_ms,
            if self.meets_execution_target() {
                "✓"
            } else {
                "✗"
            }
        ));
        report.push_str("  ─────────────────────────────\n");
        report.push_str(&format!(
            "  Total Time:         {:>6} ms\n",
            self.total_time_ms
        ));
        report.push_str(&format!(
            "  Total Overhead:     {:>6} ms\n\n",
            self.total_overhead_ms()
        ));

        report.push_str("Resource Metrics:\n");
        report.push_str(&format!(
            "  Tools Discovered:   {:>6}\n",
            self.tools_discovered
        ));
        report.push_str(&format!(
            "  Files Generated:    {:>6}\n",
            self.files_generated
        ));
        report.push_str(&format!(
            "  Code Size:          {:>6} bytes\n",
            self.generated_code_bytes
        ));
        report.push_str(&format!("  MCP Calls:          {:>6}\n", self.mcp_calls));
        report.push_str(&format!(
            "  Cache Hit:          {:>6}\n\n",
            if self.cache_hit { "Yes" } else { "No" }
        ));

        report.push_str("Token Efficiency:\n");
        report.push_str(&format!(
            "  Token Savings:      {:>5.1}% {}\n\n",
            self.token_savings_percent,
            if self.meets_token_target() {
                "✓"
            } else {
                "✗"
            }
        ));

        report.push_str("Performance Targets:\n");
        report.push_str(&format!(
            "  Execution < 50ms:   {}\n",
            if self.meets_execution_target() {
                "✓ PASS"
            } else {
                "✗ FAIL"
            }
        ));
        report.push_str(&format!(
            "  Compile < 100ms:    {}\n",
            if self.meets_compilation_target() {
                "✓ PASS"
            } else {
                "✗ FAIL"
            }
        ));
        report.push_str(&format!(
            "  Tokens ≥ 90%:       {}\n",
            if self.meets_token_target() {
                "✓ PASS"
            } else {
                "✗ FAIL"
            }
        ));

        report.push_str("\n═══════════════════════════════════════════════\n");
        report.push_str(&format!(
            "  Overall: {}\n",
            if self.meets_all_targets() {
                "✓ ALL TARGETS MET"
            } else {
                "✗ SOME TARGETS MISSED"
            }
        ));
        report.push_str("═══════════════════════════════════════════════\n");

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metrics() {
        let metrics = Metrics::new();
        assert_eq!(metrics.total_time_ms, 0);
        assert_eq!(metrics.token_savings_percent, 0.0);
        assert!(!metrics.cache_hit);
    }

    #[test]
    fn test_total_overhead() {
        let mut metrics = Metrics::new();
        metrics.introspection_time_ms = 100;
        metrics.code_generation_time_ms = 50;
        metrics.vfs_load_time_ms = 25;
        metrics.wasm_compilation_time_ms = 75;

        assert_eq!(metrics.total_overhead_ms(), 250);
    }

    #[test]
    fn test_meets_execution_target() {
        let mut metrics = Metrics::new();

        metrics.execution_time_ms = 30;
        assert!(metrics.meets_execution_target());

        metrics.execution_time_ms = 49;
        assert!(metrics.meets_execution_target());

        metrics.execution_time_ms = 50;
        assert!(!metrics.meets_execution_target());
    }

    #[test]
    fn test_meets_compilation_target() {
        let mut metrics = Metrics::new();

        metrics.wasm_compilation_time_ms = 80;
        assert!(metrics.meets_compilation_target());

        metrics.wasm_compilation_time_ms = 99;
        assert!(metrics.meets_compilation_target());

        metrics.wasm_compilation_time_ms = 100;
        assert!(!metrics.meets_compilation_target());
    }

    #[test]
    fn test_meets_token_target() {
        let mut metrics = Metrics::new();

        metrics.token_savings_percent = 92.5;
        assert!(metrics.meets_token_target());

        metrics.token_savings_percent = 90.0;
        assert!(metrics.meets_token_target());

        metrics.token_savings_percent = 89.9;
        assert!(!metrics.meets_token_target());
    }

    #[test]
    fn test_meets_all_targets() {
        let mut metrics = Metrics::new();
        metrics.execution_time_ms = 30;
        metrics.wasm_compilation_time_ms = 80;
        metrics.token_savings_percent = 92.0;

        assert!(metrics.meets_all_targets());

        metrics.execution_time_ms = 60;
        assert!(!metrics.meets_all_targets());
    }

    #[test]
    fn test_format_report() {
        let mut metrics = Metrics::new();
        metrics.total_time_ms = 250;
        metrics.execution_time_ms = 30;
        metrics.wasm_compilation_time_ms = 80;
        metrics.token_savings_percent = 92.5;

        let report = metrics.format_report();
        assert!(report.contains("PERFORMANCE METRICS REPORT"));
        assert!(report.contains("Timing Breakdown"));
        assert!(report.contains("Token Efficiency"));
        assert!(report.contains("ALL TARGETS MET"));
    }
}
