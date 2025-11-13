//! Resource monitoring for WASM execution.
//!
//! Tracks memory usage, CPU time, and execution duration to enforce
//! security limits and provide visibility into resource consumption.
//!
//! # Examples
//!
//! ```
//! use mcp_wasm_runtime::monitor::ResourceMonitor;
//! use mcp_wasm_runtime::security::SecurityConfig;
//!
//! let config = SecurityConfig::default();
//! let monitor = ResourceMonitor::new(&config);
//!
//! // Check if limits are exceeded
//! assert!(monitor.check_limits(&config).is_ok());
//! ```

use crate::security::SecurityConfig;
use mcp_core::{Error, Result};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Resource monitor for WASM execution.
///
/// Tracks resource usage and enforces limits during WASM execution.
/// Uses atomic operations for thread-safe updates.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, using atomic operations for
/// concurrent access to usage counters.
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::monitor::ResourceMonitor;
/// use mcp_wasm_runtime::security::SecurityConfig;
///
/// let config = SecurityConfig::default();
/// let monitor = ResourceMonitor::new(&config);
/// ```
#[derive(Debug)]
pub struct ResourceMonitor {
    /// Current memory usage in bytes
    memory_usage: AtomicUsize,

    /// CPU time consumed in nanoseconds
    cpu_time_ns: AtomicU64,

    /// Start time of execution
    start_time: Instant,

    /// Number of host function calls made
    host_call_count: AtomicUsize,
}

impl ResourceMonitor {
    /// Creates a new resource monitor.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// ```
    #[must_use]
    pub fn new(_config: &SecurityConfig) -> Self {
        Self {
            memory_usage: AtomicUsize::new(0),
            cpu_time_ns: AtomicU64::new(0),
            start_time: Instant::now(),
            host_call_count: AtomicUsize::new(0),
        }
    }

    /// Records memory allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// monitor.record_memory_allocation(1024);
    /// assert_eq!(monitor.memory_usage(), 1024);
    /// ```
    pub fn record_memory_allocation(&self, bytes: usize) {
        self.memory_usage.fetch_add(bytes, Ordering::Relaxed);
        tracing::trace!(
            "Memory allocated: {} bytes (total: {})",
            bytes,
            self.memory_usage()
        );
    }

    /// Records memory deallocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// monitor.record_memory_allocation(2048);
    /// monitor.record_memory_deallocation(1024);
    /// assert_eq!(monitor.memory_usage(), 1024);
    /// ```
    pub fn record_memory_deallocation(&self, bytes: usize) {
        self.memory_usage.fetch_sub(bytes, Ordering::Relaxed);
        tracing::trace!(
            "Memory deallocated: {} bytes (total: {})",
            bytes,
            self.memory_usage()
        );
    }

    /// Sets current memory usage directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// monitor.set_memory_usage(4096);
    /// assert_eq!(monitor.memory_usage(), 4096);
    /// ```
    pub fn set_memory_usage(&self, bytes: usize) {
        self.memory_usage.store(bytes, Ordering::Relaxed);
    }

    /// Returns current memory usage in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// assert_eq!(monitor.memory_usage(), 0);
    /// ```
    #[must_use]
    pub fn memory_usage(&self) -> usize {
        self.memory_usage.load(Ordering::Relaxed)
    }

    /// Returns memory usage in megabytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// monitor.set_memory_usage(10 * 1024 * 1024); // 10MB
    /// assert_eq!(monitor.memory_usage_mb(), 10);
    /// ```
    #[must_use]
    pub fn memory_usage_mb(&self) -> usize {
        self.memory_usage() / (1024 * 1024)
    }

    /// Records CPU time consumed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use std::time::Duration;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// monitor.record_cpu_time(Duration::from_millis(100));
    /// ```
    pub fn record_cpu_time(&self, duration: Duration) {
        self.cpu_time_ns
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Returns CPU time consumed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// let cpu_time = monitor.cpu_time();
    /// ```
    #[must_use]
    pub fn cpu_time(&self) -> Duration {
        Duration::from_nanos(self.cpu_time_ns.load(Ordering::Relaxed))
    }

    /// Returns elapsed time since execution start.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// let elapsed = monitor.elapsed();
    /// ```
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Returns elapsed time in milliseconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// let elapsed_ms = monitor.elapsed_ms();
    /// ```
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed().as_millis() as u64
    }

    /// Increments host function call counter.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// monitor.increment_host_calls();
    /// assert_eq!(monitor.host_call_count(), 1);
    /// ```
    pub fn increment_host_calls(&self) {
        self.host_call_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns number of host function calls made.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// assert_eq!(monitor.host_call_count(), 0);
    /// ```
    #[must_use]
    pub fn host_call_count(&self) -> usize {
        self.host_call_count.load(Ordering::Relaxed)
    }

    /// Checks if resource limits are exceeded.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Memory usage exceeds limit
    /// - Execution time exceeds timeout
    /// - Host call count exceeds limit
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// assert!(monitor.check_limits(&config).is_ok());
    /// ```
    pub fn check_limits(&self, config: &SecurityConfig) -> Result<()> {
        // Check memory limit
        let memory_bytes = self.memory_usage();
        let memory_limit = config.memory_limit_bytes();
        if memory_bytes > memory_limit {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "Memory limit exceeded: {}MB > {}MB",
                    memory_bytes / (1024 * 1024),
                    memory_limit / (1024 * 1024)
                ),
            });
        }

        // Check execution timeout
        let elapsed = self.elapsed();
        let timeout = config.execution_timeout();
        if elapsed > timeout {
            return Err(Error::Timeout {
                operation: "WASM execution".to_string(),
                duration_secs: timeout.as_secs(),
            });
        }

        // Check host call limit
        if let Some(max_calls) = config.max_host_calls() {
            let calls = self.host_call_count();
            if calls > max_calls {
                return Err(Error::SecurityViolation {
                    reason: format!("Host call limit exceeded: {} > {}", calls, max_calls),
                });
            }
        }

        Ok(())
    }

    /// Returns resource usage summary.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::monitor::ResourceMonitor;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// let monitor = ResourceMonitor::new(&config);
    /// let summary = monitor.summary();
    /// println!("{}", summary);
    /// ```
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "Memory: {}MB, CPU: {:?}, Elapsed: {}ms, Host calls: {}",
            self.memory_usage_mb(),
            self.cpu_time(),
            self.elapsed_ms(),
            self.host_call_count()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_monitor_creation() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        assert_eq!(monitor.memory_usage(), 0);
        assert_eq!(monitor.host_call_count(), 0);
    }

    #[test]
    fn test_memory_tracking() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        monitor.record_memory_allocation(1024);
        assert_eq!(monitor.memory_usage(), 1024);

        monitor.record_memory_allocation(512);
        assert_eq!(monitor.memory_usage(), 1536);

        monitor.record_memory_deallocation(256);
        assert_eq!(monitor.memory_usage(), 1280);
    }

    #[test]
    fn test_memory_mb_conversion() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        monitor.set_memory_usage(10 * 1024 * 1024); // 10MB
        assert_eq!(monitor.memory_usage_mb(), 10);
    }

    #[test]
    fn test_cpu_time_tracking() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        monitor.record_cpu_time(Duration::from_millis(100));
        monitor.record_cpu_time(Duration::from_millis(50));

        let total = monitor.cpu_time();
        assert!(total >= Duration::from_millis(150));
    }

    #[test]
    fn test_elapsed_time() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        thread::sleep(Duration::from_millis(10));

        let elapsed = monitor.elapsed();
        assert!(elapsed >= Duration::from_millis(10));
        assert!(monitor.elapsed_ms() >= 10);
    }

    #[test]
    fn test_host_call_counting() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        assert_eq!(monitor.host_call_count(), 0);

        monitor.increment_host_calls();
        assert_eq!(monitor.host_call_count(), 1);

        monitor.increment_host_calls();
        monitor.increment_host_calls();
        assert_eq!(monitor.host_call_count(), 3);
    }

    #[test]
    fn test_memory_limit_check() {
        let config = SecurityConfig::builder().memory_limit_mb(1).build();
        let monitor = ResourceMonitor::new(&config);

        // Within limit
        monitor.set_memory_usage(512 * 1024); // 512KB
        assert!(monitor.check_limits(&config).is_ok());

        // Exceed limit
        monitor.set_memory_usage(2 * 1024 * 1024); // 2MB
        assert!(monitor.check_limits(&config).is_err());
    }

    #[test]
    fn test_host_call_limit_check() {
        let config = SecurityConfig::builder().max_host_calls(5).build();
        let monitor = ResourceMonitor::new(&config);

        // Within limit
        for _ in 0..5 {
            monitor.increment_host_calls();
        }
        assert!(monitor.check_limits(&config).is_ok());

        // Exceed limit
        monitor.increment_host_calls();
        assert!(monitor.check_limits(&config).is_err());
    }

    #[test]
    fn test_summary() {
        let config = SecurityConfig::default();
        let monitor = ResourceMonitor::new(&config);

        monitor.set_memory_usage(5 * 1024 * 1024);
        monitor.increment_host_calls();

        let summary = monitor.summary();
        assert!(summary.contains("Memory: 5MB"));
        assert!(summary.contains("Host calls: 1"));
    }
}
