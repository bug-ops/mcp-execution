//! Output budgets: how many bytes of a command's output reach the caller's context.
//!
//! Output past the budget is not printed; the caller is pointed at a spill file instead (see
//! [`crate::runtime::spill`]).

use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;

use super::OutOfRange;

/// Default budget for a script's stdout, in bytes.
pub const DEFAULT_STDOUT_BUDGET_BYTES: usize = 16 * 1024;

/// Default budget for a script's stderr, in bytes.
pub const DEFAULT_STDERR_BUDGET_BYTES: usize = 4 * 1024;

/// Largest budget a caller may request, in bytes.
pub const MAX_OUTPUT_BUDGET_BYTES: usize = 1024 * 1024;

/// A non-zero byte budget of at most [`MAX_OUTPUT_BUDGET_BYTES`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::budget::OutputBudget;
///
/// let budget = OutputBudget::new(5).unwrap();
/// let cut = budget.apply("héllo wörld");
/// assert_eq!(cut.head, "héll");
/// assert!(cut.is_truncated());
/// assert!(OutputBudget::new(0).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputBudget(NonZeroUsize);

/// Text cut to an [`OutputBudget`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::budget::OutputBudget;
///
/// let cut = OutputBudget::new(100).unwrap().apply("short");
/// assert_eq!(cut.head, "short");
/// assert_eq!(cut.total_bytes, 5);
/// assert!(!cut.is_truncated());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetedText<'a> {
    /// The leading part of the text that fits the budget, cut on a character boundary.
    pub head: &'a str,
    /// Byte length of the whole text before cutting.
    pub total_bytes: usize,
}

impl BudgetedText<'_> {
    /// Returns `true` when part of the text did not fit.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::budget::OutputBudget;
    ///
    /// assert!(OutputBudget::new(1).unwrap().apply("ab").is_truncated());
    /// ```
    #[must_use]
    pub const fn is_truncated(&self) -> bool {
        self.head.len() < self.total_bytes
    }
}

impl OutputBudget {
    /// Validates `bytes` as a budget.
    ///
    /// # Errors
    ///
    /// Returns [`OutOfRange`] when `bytes` is zero or above [`MAX_OUTPUT_BUDGET_BYTES`].
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::budget::{MAX_OUTPUT_BUDGET_BYTES, OutputBudget};
    ///
    /// assert!(OutputBudget::new(MAX_OUTPUT_BUDGET_BYTES).is_ok());
    /// assert!(OutputBudget::new(MAX_OUTPUT_BUDGET_BYTES + 1).is_err());
    /// ```
    pub fn new(bytes: usize) -> Result<Self, OutOfRange> {
        NonZeroUsize::new(bytes)
            .filter(|bytes| bytes.get() <= MAX_OUTPUT_BUDGET_BYTES)
            .map(Self)
            .ok_or(OutOfRange::new(
                "output budget",
                1,
                MAX_OUTPUT_BUDGET_BYTES as u64,
            ))
    }

    /// The default stdout budget.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::budget::OutputBudget;
    ///
    /// assert_eq!(OutputBudget::stdout_default().get(), 16 * 1024);
    /// ```
    #[must_use]
    pub const fn stdout_default() -> Self {
        Self::from_const(DEFAULT_STDOUT_BUDGET_BYTES)
    }

    /// The default stderr budget.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::budget::OutputBudget;
    ///
    /// assert_eq!(OutputBudget::stderr_default().get(), 4 * 1024);
    /// ```
    #[must_use]
    pub const fn stderr_default() -> Self {
        Self::from_const(DEFAULT_STDERR_BUDGET_BYTES)
    }

    const fn from_const(bytes: usize) -> Self {
        match NonZeroUsize::new(bytes) {
            Some(bytes) => Self(bytes),
            None => panic!("default budget constants are non-zero"),
        }
    }

    /// Returns the budget in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::budget::OutputBudget;
    ///
    /// assert_eq!(OutputBudget::new(10).unwrap().get(), 10);
    /// ```
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Cuts `text` to this budget on a character boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::budget::OutputBudget;
    ///
    /// let cut = OutputBudget::new(2).unwrap().apply("日本");
    /// assert_eq!(cut.head, "");
    /// assert_eq!(cut.total_bytes, 6);
    /// ```
    #[must_use]
    pub fn apply(self, text: &str) -> BudgetedText<'_> {
        let end = text.floor_char_boundary(self.get());
        BudgetedText {
            head: &text[..end],
            total_bytes: text.len(),
        }
    }
}

impl fmt::Display for OutputBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for OutputBudget {
    type Err = OutOfRange;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        raw.parse::<usize>()
            .map_err(|_| OutOfRange::new("output budget", 1, MAX_OUTPUT_BUDGET_BYTES as u64))
            .and_then(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_never_splits_a_character() {
        let budget = OutputBudget::new(4).unwrap();
        for text in ["aé日x", "日本語", "😀😀"] {
            let cut = budget.apply(text);
            assert!(text.starts_with(cut.head));
            assert!(cut.head.len() <= 4);
        }
    }

    #[test]
    fn parse_rejects_non_numeric_and_out_of_range() {
        assert!("abc".parse::<OutputBudget>().is_err());
        assert!("-1".parse::<OutputBudget>().is_err());
        assert!("0".parse::<OutputBudget>().is_err());
        assert_eq!("4096".parse::<OutputBudget>().unwrap().get(), 4096);
    }

    #[test]
    fn defaults_are_inside_the_allowed_range() {
        assert!(OutputBudget::new(OutputBudget::stdout_default().get()).is_ok());
        assert!(OutputBudget::new(OutputBudget::stderr_default().get()).is_ok());
    }
}
