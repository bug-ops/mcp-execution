//! Code execution trait.
//!
//! This module defines the `CodeExecutor` trait, which provides the interface
//! for executing code in a sandboxed environment with resource limits.

use crate::{MemoryLimit, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

/// Executes code in a sandboxed environment.
///
/// Implementations of this trait provide code execution capabilities with
/// configurable resource limits (memory, time) and security boundaries.
///
/// # Type Safety
///
/// All implementations must be `Send + Sync` to work with Tokio's async runtime.
///
/// # Examples
///
/// ```
/// use mcp_core::traits::CodeExecutor;
/// use mcp_core::{MemoryLimit, Result, Error};
/// use async_trait::async_trait;
/// use serde_json::Value;
/// use std::time::Duration;
///
/// struct SimpleExecutor {
///     memory: MemoryLimit,
///     timeout: Duration,
/// }
///
/// #[async_trait]
/// impl CodeExecutor for SimpleExecutor {
///     async fn execute(&mut self, code: &str) -> Result<Value> {
///         // Simulate execution
///         if code.contains("error") {
///             return Err(Error::ExecutionError {
///                 message: "Code contains error".to_string(),
///                 source: None,
///             });
///         }
///         Ok(Value::String("success".to_string()))
///     }
///
///     fn set_memory_limit(&mut self, limit: MemoryLimit) {
///         self.memory = limit;
///     }
///
///     fn set_timeout(&mut self, timeout: Duration) {
///         self.timeout = timeout;
///     }
///
///     fn memory_limit(&self) -> MemoryLimit {
///         self.memory
///     }
///
///     fn timeout(&self) -> Duration {
///         self.timeout
///     }
/// }
/// ```
#[async_trait]
pub trait CodeExecutor: Send + Sync {
    /// Executes code and returns the result.
    ///
    /// The code is executed in a sandboxed environment with the configured
    /// memory limits and timeout. The result is returned as a JSON value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Execution fails or panics
    /// - Memory limit is exceeded
    /// - Execution timeout is reached
    /// - Security policy is violated
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CodeExecutor;
    /// # use mcp_core::Result;
    /// # async fn example(executor: &mut impl CodeExecutor) -> Result<()> {
    /// let code = "console.log('Hello, world!')";
    /// let result = executor.execute(code).await?;
    /// println!("Result: {}", result);
    /// # Ok(())
    /// # }
    /// ```
    async fn execute(&mut self, code: &str) -> Result<Value>;

    /// Sets the memory limit for execution.
    ///
    /// This limit applies to the next execution and persists until changed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CodeExecutor;
    /// # use mcp_core::MemoryLimit;
    /// # fn example(executor: &mut impl CodeExecutor) {
    /// executor.set_memory_limit(MemoryLimit::from_mb(512).unwrap());
    /// # }
    /// ```
    fn set_memory_limit(&mut self, limit: MemoryLimit);

    /// Sets the execution timeout.
    ///
    /// This timeout applies to the next execution and persists until changed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CodeExecutor;
    /// # use std::time::Duration;
    /// # fn example(executor: &mut impl CodeExecutor) {
    /// executor.set_timeout(Duration::from_secs(60));
    /// # }
    /// ```
    fn set_timeout(&mut self, timeout: Duration);

    /// Returns the current memory limit.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CodeExecutor;
    /// # fn example(executor: &impl CodeExecutor) {
    /// let limit = executor.memory_limit();
    /// println!("Memory limit: {}MB", limit.megabytes());
    /// # }
    /// ```
    fn memory_limit(&self) -> MemoryLimit;

    /// Returns the current execution timeout.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CodeExecutor;
    /// # fn example(executor: &impl CodeExecutor) {
    /// let timeout = executor.timeout();
    /// println!("Timeout: {}s", timeout.as_secs());
    /// # }
    /// ```
    fn timeout(&self) -> Duration;

    /// Resets the executor to its default state.
    ///
    /// This method clears any cached state and resets resource limits
    /// to their default values.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CodeExecutor;
    /// # fn example(executor: &mut impl CodeExecutor) {
    /// executor.reset();
    /// # }
    /// ```
    fn reset(&mut self) {
        self.set_memory_limit(MemoryLimit::default());
        self.set_timeout(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    struct TestExecutor {
        memory: MemoryLimit,
        timeout: Duration,
    }

    impl TestExecutor {
        fn new() -> Self {
            Self {
                memory: MemoryLimit::default(),
                timeout: Duration::from_secs(30),
            }
        }
    }

    #[async_trait]
    impl CodeExecutor for TestExecutor {
        async fn execute(&mut self, code: &str) -> Result<Value> {
            if code.is_empty() {
                return Err(Error::ExecutionError {
                    message: "Empty code".to_string(),
                    source: None,
                });
            }
            Ok(Value::String(format!("executed: {code}")))
        }

        fn set_memory_limit(&mut self, limit: MemoryLimit) {
            self.memory = limit;
        }

        fn set_timeout(&mut self, timeout: Duration) {
            self.timeout = timeout;
        }

        fn memory_limit(&self) -> MemoryLimit {
            self.memory
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }
    }

    #[tokio::test]
    async fn test_executor_execute() {
        let mut executor = TestExecutor::new();
        let result = executor.execute("test code").await.unwrap();
        assert_eq!(result, Value::String("executed: test code".to_string()));
    }

    #[tokio::test]
    async fn test_executor_error() {
        let mut executor = TestExecutor::new();
        let result = executor.execute("").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_execution_error());
    }

    #[test]
    fn test_set_memory_limit() {
        let mut executor = TestExecutor::new();
        let new_limit = MemoryLimit::from_mb(512).unwrap();

        executor.set_memory_limit(new_limit);
        assert_eq!(executor.memory_limit(), new_limit);
    }

    #[test]
    fn test_set_timeout() {
        let mut executor = TestExecutor::new();
        let new_timeout = Duration::from_secs(60);

        executor.set_timeout(new_timeout);
        assert_eq!(executor.timeout(), new_timeout);
    }

    #[test]
    fn test_reset() {
        let mut executor = TestExecutor::new();

        // Change values
        executor.set_memory_limit(MemoryLimit::from_mb(512).unwrap());
        executor.set_timeout(Duration::from_secs(120));

        // Reset
        executor.reset();

        // Should be back to defaults
        assert_eq!(executor.memory_limit(), MemoryLimit::default());
        assert_eq!(executor.timeout(), Duration::from_secs(30));
    }
}
