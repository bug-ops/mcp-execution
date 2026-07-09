//! Injectable clock abstraction for session expiry.
//!
//! Production code uses [`SystemClock`], a zero-cost wrapper around
//! [`Utc::now`](chrono::Utc::now). Tests can inject a fake clock to
//! deterministically exercise expiry boundaries instead of rewinding
//! [`PendingGeneration::expires_at`](crate::types::PendingGeneration::expires_at)
//! after construction.

use chrono::{DateTime, Utc};
use std::fmt::Debug;

/// Provides the current time for session expiry calculations.
///
/// Implementors must be [`Send`] + [`Sync`] so they can be shared across
/// async tasks via `Arc<dyn Clock>`.
///
/// # Examples
///
/// ```
/// use mcp_execution_server::clock::{Clock, SystemClock};
///
/// let clock = SystemClock;
/// assert!(clock.now().timestamp() > 0);
/// ```
pub trait Clock: Send + Sync + Debug {
    /// Returns the current time.
    fn now(&self) -> DateTime<Utc>;
}

/// Real-time [`Clock`] backed by [`Utc::now`].
///
/// This is the default clock used in production; it matches the previous
/// unconditional `Utc::now()` calls exactly.
///
/// # Examples
///
/// ```
/// use mcp_execution_server::clock::{Clock, SystemClock};
///
/// let clock = SystemClock;
/// assert!(clock.now().timestamp() > 0);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[cfg(test)]
pub use test_support::TestClock;

#[cfg(test)]
mod test_support {
    use super::Clock;
    use chrono::{DateTime, Duration, Utc};
    use std::sync::{Arc, Mutex};

    /// Fake clock for tests: holds a mutable timestamp that can be advanced
    /// or set directly, so expiry boundaries can be exercised deterministically
    /// without rewinding `expires_at` after construction.
    ///
    /// Cloning shares the underlying timestamp (`Arc<Mutex<_>>`), so a clone
    /// handed to a `StateManager` or `PendingGeneration` observes the same
    /// advances as the original.
    #[derive(Debug, Clone)]
    pub struct TestClock(Arc<Mutex<DateTime<Utc>>>);

    impl TestClock {
        /// Creates a fake clock fixed at `now`.
        #[must_use]
        pub fn new(now: DateTime<Utc>) -> Self {
            Self(Arc::new(Mutex::new(now)))
        }

        /// Sets the clock to an arbitrary point in time.
        ///
        /// # Panics
        ///
        /// Panics if the internal mutex is poisoned.
        pub fn set(&self, now: DateTime<Utc>) {
            *self.0.lock().unwrap() = now;
        }

        /// Advances the clock forward by `duration`.
        ///
        /// # Panics
        ///
        /// Panics if the internal mutex is poisoned.
        pub fn advance(&self, duration: Duration) {
            let mut guard = self.0.lock().unwrap();
            *guard += duration;
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> DateTime<Utc> {
            *self.0.lock().unwrap()
        }
    }
}
