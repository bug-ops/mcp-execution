//! Runner notices that a script cannot forge.
//!
//! Every notice the runner prints carries a per-run token the script never receives, and the
//! first stdout line is written before the script process exists, so a consumer can tell runner
//! text from script text.

/// A per-run unguessable token tagging every runner notice.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::notice::RunToken;
///
/// let token = RunToken::generate();
/// assert!(token.anchor_line().contains(token.as_str()));
/// assert!(token.line("note").starts_with("[mcp-execution "));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunToken(String);

impl RunToken {
    /// Generates a fresh 128-bit token (32 lowercase hex characters).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::notice::RunToken;
    ///
    /// assert_ne!(RunToken::generate(), RunToken::generate());
    /// ```
    #[must_use]
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    /// The token text.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::notice::RunToken;
    ///
    /// assert_eq!(RunToken::generate().as_str().len(), 32);
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Tags `text` as a runner notice: `[mcp-execution <token>] <text>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::notice::RunToken;
    ///
    /// let token = RunToken::generate();
    /// assert_eq!(token.line("hi"), format!("[mcp-execution {}] hi", token.as_str()));
    /// ```
    #[must_use]
    pub fn line(&self, text: &str) -> String {
        format!("[mcp-execution {}] {text}", self.0)
    }

    /// The first stdout line of a run: `[mcp-execution <token>] run started`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::notice::RunToken;
    ///
    /// assert!(RunToken::generate().anchor_line().ends_with("] run started"));
    /// ```
    #[must_use]
    pub fn anchor_line(&self) -> String {
        self.line("run started")
    }
}
