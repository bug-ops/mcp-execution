//! Strong domain types for MCP Code Execution.
//!
//! This module implements the newtype pattern to provide type safety for
//! domain primitives, following ADR-003 (strong types over primitives).
//!
//! # Type Safety Benefits
//!
//! Using strong types instead of primitives prevents:
//! - Mixing up parameters of the same primitive type
//! - Invalid values being passed
//! - Accidental type conversions
//!
//! # Skill Generation
//!
//! This project generates skills **exclusively** in the Anthropic Claude Agent Skills format:
//! - Skills are generated in `.claude/skills/skill-name/` directory
//! - Each skill has a `SKILL.md` file with YAML frontmatter
//! - Follows Anthropic's specification for Claude Code/Desktop integration
//! - No legacy format support, no backward compatibility concerns
//!
//! # Examples
//!
//! ```
//! use mcp_core::{ServerId, SessionId, MemoryLimit, SkillName, SkillDescription};
//!
//! // Type-safe identifiers
//! let server = ServerId::new("my-server");
//! let session = SessionId::generate();
//!
//! // Type-safe memory limits
//! let limit = MemoryLimit::default();
//! assert_eq!(limit.bytes(), 256 * 1024 * 1024);
//!
//! // Validated Anthropic skill types
//! let name = SkillName::new("vkteams-bot").unwrap();
//! let desc = SkillDescription::new("Sends messages to VK Teams. Use when...").unwrap();
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

/// Server identifier (newtype over String).
///
/// Represents a unique identifier for an MCP server. Using a strong type
/// prevents accidentally mixing server IDs with other string values.
///
/// # Examples
///
/// ```
/// use mcp_core::ServerId;
///
/// let id = ServerId::new("example-server");
/// assert_eq!(id.as_str(), "example-server");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerId(String);

impl ServerId {
    /// Creates a new server identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ServerId;
    ///
    /// let id = ServerId::new("my-server");
    /// let from_string = ServerId::new(String::from("my-server"));
    /// assert_eq!(id, from_string);
    /// ```
    #[inline]
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the server ID as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ServerId;
    ///
    /// let id = ServerId::new("test");
    /// assert_eq!(id.as_str(), "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `ServerId` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ServerId;
    ///
    /// let id = ServerId::new("test");
    /// let inner: String = id.into_inner();
    /// assert_eq!(inner, "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ServerId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ServerId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Tool name identifier (newtype over String).
///
/// Represents the name of an MCP tool. Using a strong type ensures
/// tool names are not confused with other string values.
///
/// # Examples
///
/// ```
/// use mcp_core::ToolName;
///
/// let tool = ToolName::new("execute_code");
/// assert_eq!(tool.as_str(), "execute_code");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolName(String);

impl ToolName {
    /// Creates a new tool name.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ToolName;
    ///
    /// let name = ToolName::new("my_tool");
    /// assert_eq!(name.as_str(), "my_tool");
    /// ```
    #[inline]
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the tool name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ToolName;
    ///
    /// let name = ToolName::new("test_tool");
    /// assert_eq!(name.as_str(), "test_tool");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `ToolName` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ToolName;
    ///
    /// let name = ToolName::new("tool");
    /// let inner: String = name.into_inner();
    /// assert_eq!(inner, "tool");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ToolName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Session identifier for WASM execution (newtype over String).
///
/// Represents a unique execution session. Each WASM execution gets a unique
/// session ID to track state and isolate executions.
///
/// # Examples
///
/// ```
/// use mcp_core::SessionId;
///
/// // Generate unique IDs
/// let id1 = SessionId::generate();
/// let id2 = SessionId::generate();
/// assert_ne!(id1, id2);
///
/// // Create from string
/// let custom = SessionId::new("custom-session");
/// assert_eq!(custom.as_str(), "custom-session");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Creates a new session identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SessionId;
    ///
    /// let id = SessionId::new("session-123");
    /// assert_eq!(id.as_str(), "session-123");
    /// ```
    #[inline]
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generates a new unique session identifier.
    ///
    /// Uses UUID v4 (random) to ensure cryptographically secure uniqueness.
    /// This method is suitable for production use with distributed systems.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SessionId;
    ///
    /// let id1 = SessionId::generate();
    /// let id2 = SessionId::generate();
    /// assert_ne!(id1, id2);
    /// assert!(id1.as_str().starts_with("session_"));
    /// ```
    #[must_use]
    pub fn generate() -> Self {
        use uuid::Uuid;
        Self(format!("session_{}", Uuid::new_v4()))
    }

    /// Returns the session ID as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SessionId;
    ///
    /// let id = SessionId::new("test-session");
    /// assert_eq!(id.as_str(), "test-session");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `SessionId` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SessionId;
    ///
    /// let id = SessionId::new("test");
    /// let inner: String = id.into_inner();
    /// assert_eq!(inner, "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Memory limit with type safety (newtype over usize).
///
/// Represents memory limits for WASM execution in bytes. Using a strong type
/// ensures memory limits are not confused with other numeric values and
/// enforces validation.
///
/// # Examples
///
/// ```
/// use mcp_core::MemoryLimit;
///
/// // Use default (256MB)
/// let default = MemoryLimit::default();
/// assert_eq!(default.bytes(), 256 * 1024 * 1024);
///
/// // Create custom limit
/// let custom = MemoryLimit::new(100 * 1024 * 1024).unwrap();
/// assert_eq!(custom.bytes(), 100 * 1024 * 1024);
///
/// // Exceeding max fails
/// let too_large = MemoryLimit::new(1024 * 1024 * 1024);
/// assert!(too_large.is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MemoryLimit(usize);

impl MemoryLimit {
    /// Default memory limit: 256MB.
    pub const DEFAULT: Self = Self(256 * 1024 * 1024);

    /// Maximum allowed memory limit: 512MB.
    pub const MAX: Self = Self(512 * 1024 * 1024);

    /// Minimum memory limit: 1MB.
    pub const MIN: Self = Self(1024 * 1024);

    /// Creates a new memory limit with validation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The limit exceeds `MAX` (512MB)
    /// - The limit is below `MIN` (1MB)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::MemoryLimit;
    ///
    /// // Valid limits
    /// let small = MemoryLimit::new(10 * 1024 * 1024).unwrap();
    /// let large = MemoryLimit::new(500 * 1024 * 1024).unwrap();
    ///
    /// // Invalid limits
    /// assert!(MemoryLimit::new(1024).is_err()); // Too small
    /// assert!(MemoryLimit::new(1024 * 1024 * 1024).is_err()); // Too large
    /// ```
    pub const fn new(bytes: usize) -> Result<Self, &'static str> {
        if bytes < Self::MIN.0 {
            Err("Memory limit below minimum (1MB)")
        } else if bytes > Self::MAX.0 {
            Err("Memory limit exceeds maximum (512MB)")
        } else {
            Ok(Self(bytes))
        }
    }

    /// Creates a memory limit from megabytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the resulting byte value is out of valid range.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::MemoryLimit;
    ///
    /// let limit = MemoryLimit::from_mb(128).unwrap();
    /// assert_eq!(limit.bytes(), 128 * 1024 * 1024);
    /// ```
    pub const fn from_mb(megabytes: usize) -> Result<Self, &'static str> {
        Self::new(megabytes * 1024 * 1024)
    }

    /// Returns the memory limit in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::MemoryLimit;
    ///
    /// let limit = MemoryLimit::default();
    /// assert_eq!(limit.bytes(), 256 * 1024 * 1024);
    /// ```
    #[inline]
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.0
    }

    /// Returns the memory limit in megabytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::MemoryLimit;
    ///
    /// let limit = MemoryLimit::default();
    /// assert_eq!(limit.megabytes(), 256);
    /// ```
    #[inline]
    #[must_use]
    pub const fn megabytes(&self) -> usize {
        self.0 / (1024 * 1024)
    }
}

impl Default for MemoryLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for MemoryLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}MB", self.megabytes())
    }
}

/// Cache key for storing and retrieving cached results (newtype over String).
///
/// Represents a unique key for caching tool call results. Using a strong type
/// ensures cache keys are not confused with other string values. Keys created
/// from parts are collision-resistant hashes.
///
/// # Examples
///
/// ```
/// use mcp_core::CacheKey;
///
/// // Create from components (produces a hash)
/// let key = CacheKey::from_parts("server", "tool", r#"{"arg": "value"}"#);
/// assert!(key.as_str().starts_with("cache_"));
///
/// // Create custom key
/// let custom = CacheKey::new("custom-cache-key");
/// assert_eq!(custom.as_str(), "custom-cache-key");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey(String);

impl CacheKey {
    /// Creates a new cache key from a string.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheKey;
    ///
    /// let key = CacheKey::new("my-cache-key");
    /// assert_eq!(key.as_str(), "my-cache-key");
    /// ```
    #[inline]
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Creates a cache key from server, tool, and parameters.
    ///
    /// This method generates a consistent, collision-resistant cache key by hashing
    /// the server ID, tool name, and parameters using BLAKE3. Each component is
    /// separated with null bytes to prevent injection attacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheKey;
    ///
    /// let key = CacheKey::from_parts(
    ///     "vkteams-bot",
    ///     "send_message",
    ///     r#"{"chat_id": "123", "text": "hello"}"#
    /// );
    ///
    /// // Same inputs always produce the same hash
    /// let key2 = CacheKey::from_parts(
    ///     "vkteams-bot",
    ///     "send_message",
    ///     r#"{"chat_id": "123", "text": "hello"}"#
    /// );
    /// assert_eq!(key, key2);
    ///
    /// // Different inputs produce different hashes
    /// let key3 = CacheKey::from_parts("other", "send_message", "{}");
    /// assert_ne!(key, key3);
    /// ```
    #[must_use]
    pub fn from_parts(server: &str, tool: &str, params: &str) -> Self {
        use blake3::Hasher;

        let mut hasher = Hasher::new();
        hasher.update(server.as_bytes());
        hasher.update(b"\0"); // Null byte separator prevents injection
        hasher.update(tool.as_bytes());
        hasher.update(b"\0");
        hasher.update(params.as_bytes());

        let hash = hasher.finalize();
        Self(format!("cache_{}", hash.to_hex()))
    }

    /// Returns the cache key as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheKey;
    ///
    /// let key = CacheKey::new("test-key");
    /// assert_eq!(key.as_str(), "test-key");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `CacheKey` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheKey;
    ///
    /// let key = CacheKey::new("test");
    /// let inner: String = key.into_inner();
    /// assert_eq!(inner, "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for CacheKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CacheKey {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Validated skill description for Anthropic Claude Agent Skills format.
///
/// Skill descriptions must follow strict validation rules per the Anthropic specification:
/// - Maximum 1024 characters
/// - No XML tags (`<` or `>`)
/// - No template syntax (`{{` or `}}`)
/// - Third-person voice recommended (not enforced)
/// - Should include activation triggers (when to use the skill)
///
/// # Examples
///
/// ```
/// use mcp_core::SkillDescription;
///
/// // Valid description
/// let desc = SkillDescription::new(
///     "Interact with VK Teams messenger to send messages, create chats, \
///      and manage groups. Use when working with VK Teams or when user \
///      mentions VK messenger integration."
/// ).unwrap();
/// assert!(desc.as_str().len() <= 1024);
///
/// // Too long
/// let long = "x".repeat(1025);
/// assert!(SkillDescription::new(&long).is_err());
///
/// // XML tags forbidden
/// assert!(SkillDescription::new("<script>alert(1)</script>").is_err());
///
/// // Template syntax forbidden
/// assert!(SkillDescription::new("Bad {{injection}}").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillDescription(String);

impl SkillDescription {
    /// Maximum allowed description length (Anthropic specification).
    pub const MAX_LENGTH: usize = 1024;

    /// Creates a new validated skill description.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Description is empty
    /// - Length exceeds `MAX_LENGTH` (1024 characters)
    /// - Contains XML tags (`<` or `>`)
    /// - Contains template syntax (`{{` or `}}`)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillDescription;
    ///
    /// // Valid description
    /// let desc = SkillDescription::new("Sends messages to VK Teams. Use when...").unwrap();
    ///
    /// // Invalid: too long
    /// let result = SkillDescription::new(&"x".repeat(1025));
    /// assert!(result.is_err());
    ///
    /// // Invalid: XML tags
    /// let result = SkillDescription::new("<script>bad</script>");
    /// assert!(result.is_err());
    ///
    /// // Invalid: template syntax
    /// let result = SkillDescription::new("Bad {{injection}}");
    /// assert!(result.is_err());
    /// ```
    pub fn new(description: impl AsRef<str>) -> crate::Result<Self> {
        let desc = description.as_ref();

        if desc.is_empty() {
            return Err(crate::Error::ValidationError {
                field: "description".to_string(),
                reason: "Description cannot be empty".to_string(),
            });
        }

        if desc.len() > Self::MAX_LENGTH {
            return Err(crate::Error::ValidationError {
                field: "description".to_string(),
                reason: format!(
                    "Description too long ({} > {} characters)",
                    desc.len(),
                    Self::MAX_LENGTH
                ),
            });
        }

        // Check for XML tags
        if desc.contains('<') || desc.contains('>') {
            return Err(crate::Error::ValidationError {
                field: "description".to_string(),
                reason: "Description cannot contain XML tags (< or >)".to_string(),
            });
        }

        // Check for template syntax
        if desc.contains("{{") || desc.contains("}}") {
            return Err(crate::Error::ValidationError {
                field: "description".to_string(),
                reason: "Description cannot contain template syntax ({{ or }})".to_string(),
            });
        }

        Ok(Self(desc.to_string()))
    }

    /// Returns the description as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillDescription;
    ///
    /// let desc = SkillDescription::new("Test description").unwrap();
    /// assert_eq!(desc.as_str(), "Test description");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `SkillDescription` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillDescription;
    ///
    /// let desc = SkillDescription::new("Test").unwrap();
    /// let inner: String = desc.into_inner();
    /// assert_eq!(inner, "Test");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SkillDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for SkillDescription {
    type Error = crate::Error;

    fn try_from(s: String) -> crate::Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<&str> for SkillDescription {
    type Error = crate::Error;

    fn try_from(s: &str) -> crate::Result<Self> {
        Self::new(s)
    }
}

/// Skill name identifier with Anthropic Claude Agent Skills format validation.
///
/// Skill names must follow strict validation rules per the Anthropic specification:
/// - Length: 1-64 characters
/// - Characters: Only lowercase letters (a-z), numbers (0-9), hyphens (-), underscores (_)
/// - Must start with a lowercase letter
/// - Must end with a lowercase letter or number
/// - Reserved words forbidden: "anthropic", "claude" (case-insensitive)
/// - No XML tags
///
/// # Examples
///
/// ```
/// use mcp_core::SkillName;
///
/// // Valid names
/// assert!(SkillName::new("vkteams-bot").is_ok());
/// assert!(SkillName::new("my_skill_123").is_ok());
/// assert!(SkillName::new("a").is_ok());
///
/// // Invalid: reserved word
/// assert!(SkillName::new("anthropic-skill").is_err());
/// assert!(SkillName::new("claude-bot").is_err());
///
/// // Invalid: uppercase
/// assert!(SkillName::new("MySkill").is_err());
///
/// // Invalid: starts with number
/// assert!(SkillName::new("123skill").is_err());
///
/// // Invalid: ends with hyphen
/// assert!(SkillName::new("skill-").is_err());
///
/// // Invalid: too long
/// assert!(SkillName::new(&"x".repeat(65)).is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillName(String);

impl SkillName {
    /// Minimum allowed skill name length.
    pub const MIN_LENGTH: usize = 1;

    /// Maximum allowed skill name length (Anthropic specification).
    pub const MAX_LENGTH: usize = 64;

    /// Reserved words that cannot appear in skill names (case-insensitive).
    pub const RESERVED_WORDS: &'static [&'static str] = &["anthropic", "claude"];

    /// Creates a new validated skill name.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Name is empty or exceeds 64 characters
    /// - Contains reserved words ("anthropic", "claude")
    /// - Contains XML tags
    /// - Contains invalid characters (only a-z, 0-9, -, _ allowed)
    /// - Does not start with a lowercase letter
    /// - Does not end with a letter or number
    ///
    /// # Panics
    ///
    /// This function does not panic. The `.unwrap()` calls on `chars().next()`
    /// and `chars().last()` are safe because we check for empty strings first.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillName;
    ///
    /// // Valid names
    /// let name1 = SkillName::new("vkteams-bot").unwrap();
    /// let name2 = SkillName::new("my_skill_123").unwrap();
    ///
    /// // Invalid: reserved word
    /// assert!(SkillName::new("anthropic-helper").is_err());
    ///
    /// // Invalid: uppercase
    /// assert!(SkillName::new("MySkill").is_err());
    ///
    /// // Invalid: starts with number
    /// assert!(SkillName::new("123skill").is_err());
    /// ```
    pub fn new(name: impl AsRef<str>) -> crate::Result<Self> {
        let name = name.as_ref();

        // Check length
        if name.is_empty() {
            return Err(crate::Error::ValidationError {
                field: "skill_name".to_string(),
                reason: "Skill name cannot be empty".to_string(),
            });
        }

        if name.len() > Self::MAX_LENGTH {
            return Err(crate::Error::ValidationError {
                field: "skill_name".to_string(),
                reason: format!(
                    "Skill name too long ({} > {} characters)",
                    name.len(),
                    Self::MAX_LENGTH
                ),
            });
        }

        // Check reserved words (case-insensitive)
        let lowercase_name = name.to_lowercase();
        for reserved in Self::RESERVED_WORDS {
            if lowercase_name.contains(reserved) {
                return Err(crate::Error::ReservedWord {
                    name: name.to_string(),
                    reserved_word: (*reserved).to_string(),
                });
            }
        }

        // Check for XML tags
        if name.contains('<') || name.contains('>') {
            return Err(crate::Error::ValidationError {
                field: "skill_name".to_string(),
                reason: "Skill name cannot contain XML tags (< or >)".to_string(),
            });
        }

        // Check first character (must be lowercase letter)
        let first_char = name.chars().next().unwrap();
        if !first_char.is_ascii_lowercase() {
            return Err(crate::Error::ValidationError {
                field: "skill_name".to_string(),
                reason: format!(
                    "Skill name must start with a lowercase letter, got '{first_char}'"
                ),
            });
        }

        // Check last character (must be letter or number)
        let last_char = name.chars().last().unwrap();
        if !last_char.is_ascii_lowercase() && !last_char.is_ascii_digit() {
            return Err(crate::Error::ValidationError {
                field: "skill_name".to_string(),
                reason: format!("Skill name must end with a letter or number, got '{last_char}'"),
            });
        }

        // Check all characters (only a-z, 0-9, hyphens, underscores)
        for ch in name.chars() {
            if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' && ch != '_' {
                return Err(crate::Error::ValidationError {
                    field: "skill_name".to_string(),
                    reason: format!(
                        "Invalid character '{ch}'. Only lowercase letters, numbers, hyphens, and underscores allowed"
                    ),
                });
            }
        }

        Ok(Self(name.to_string()))
    }

    /// Returns the skill name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillName;
    ///
    /// let name = SkillName::new("test-skill").unwrap();
    /// assert_eq!(name.as_str(), "test-skill");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `SkillName` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillName;
    ///
    /// let name = SkillName::new("test").unwrap();
    /// let inner: String = name.into_inner();
    /// assert_eq!(inner, "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SkillName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for SkillName {
    type Error = crate::Error;

    fn try_from(s: String) -> crate::Result<Self> {
        Self::new(s)
    }
}

impl TryFrom<&str> for SkillName {
    type Error = crate::Error;

    fn try_from(s: &str) -> crate::Result<Self> {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_id_creation() {
        let id = ServerId::new("test-server");
        assert_eq!(id.as_str(), "test-server");
        assert_eq!(id.to_string(), "test-server");
    }

    #[test]
    fn test_server_id_from_string() {
        let id: ServerId = "test".into();
        assert_eq!(id.as_str(), "test");

        let id2: ServerId = String::from("test2").into();
        assert_eq!(id2.as_str(), "test2");
    }

    #[test]
    fn test_server_id_equality() {
        let id1 = ServerId::new("server");
        let id2 = ServerId::new("server");
        let id3 = ServerId::new("other");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_tool_name_creation() {
        let name = ToolName::new("execute_code");
        assert_eq!(name.as_str(), "execute_code");
        assert_eq!(name.to_string(), "execute_code");
    }

    #[test]
    fn test_tool_name_from_str() {
        let name: ToolName = "tool".into();
        assert_eq!(name.as_str(), "tool");
    }

    #[test]
    fn test_session_id_creation() {
        let id = SessionId::new("custom-session");
        assert_eq!(id.as_str(), "custom-session");
    }

    #[test]
    fn test_session_id_generation() {
        let id1 = SessionId::generate();
        let id2 = SessionId::generate();
        let id3 = SessionId::generate();

        // All should be unique
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        // Should start with prefix
        assert!(id1.as_str().starts_with("session_"));
        assert!(id2.as_str().starts_with("session_"));
        assert!(id3.as_str().starts_with("session_"));
    }

    #[test]
    fn test_memory_limit_default() {
        let limit = MemoryLimit::default();
        assert_eq!(limit.bytes(), 256 * 1024 * 1024);
        assert_eq!(limit.megabytes(), 256);
    }

    #[test]
    fn test_memory_limit_creation() {
        let limit = MemoryLimit::new(100 * 1024 * 1024).unwrap();
        assert_eq!(limit.bytes(), 100 * 1024 * 1024);
        assert_eq!(limit.megabytes(), 100);
    }

    #[test]
    fn test_memory_limit_from_mb() {
        let limit = MemoryLimit::from_mb(128).unwrap();
        assert_eq!(limit.megabytes(), 128);
        assert_eq!(limit.bytes(), 128 * 1024 * 1024);
    }

    #[test]
    fn test_memory_limit_validation() {
        // Valid limits
        assert!(MemoryLimit::new(1024 * 1024).is_ok());
        assert!(MemoryLimit::new(512 * 1024 * 1024).is_ok());

        // Too small
        assert!(MemoryLimit::new(1024).is_err());
        assert!(MemoryLimit::new(512 * 1024).is_err());

        // Too large
        assert!(MemoryLimit::new(513 * 1024 * 1024).is_err());
        assert!(MemoryLimit::new(1024 * 1024 * 1024).is_err());
    }

    #[test]
    fn test_memory_limit_constants() {
        assert_eq!(MemoryLimit::DEFAULT.bytes(), 256 * 1024 * 1024);
        assert_eq!(MemoryLimit::MAX.bytes(), 512 * 1024 * 1024);
        assert_eq!(MemoryLimit::MIN.bytes(), 1024 * 1024);
    }

    #[test]
    fn test_memory_limit_display() {
        let limit = MemoryLimit::default();
        assert_eq!(format!("{limit}"), "256MB");

        let custom = MemoryLimit::new(100 * 1024 * 1024).unwrap();
        assert_eq!(format!("{custom}"), "100MB");
    }

    #[test]
    fn test_memory_limit_ordering() {
        let small = MemoryLimit::new(10 * 1024 * 1024).unwrap();
        let large = MemoryLimit::new(20 * 1024 * 1024).unwrap();

        assert!(small < large);
        assert!(large > small);
    }

    #[test]
    fn test_into_inner() {
        let server_id = ServerId::new("test");
        assert_eq!(server_id.into_inner(), "test");

        let tool_name = ToolName::new("tool");
        assert_eq!(tool_name.into_inner(), "tool");

        let session_id = SessionId::new("session");
        assert_eq!(session_id.into_inner(), "session");
    }

    #[test]
    fn test_cache_key_creation() {
        let key = CacheKey::new("test-cache-key");
        assert_eq!(key.as_str(), "test-cache-key");
        assert_eq!(key.to_string(), "test-cache-key");
    }

    #[test]
    fn test_cache_key_from_parts() {
        let key = CacheKey::from_parts("server", "tool", r#"{"arg": "value"}"#);
        let key_str = key.as_str();

        // Key should be a hash (cache_ prefix + 64 hex chars)
        assert!(key_str.starts_with("cache_"));
        assert_eq!(key_str.len(), 6 + 64); // "cache_" + 64 hex chars
    }

    #[test]
    fn test_cache_key_from_parts_consistency() {
        let key1 = CacheKey::from_parts("srv", "tool", "params");
        let key2 = CacheKey::from_parts("srv", "tool", "params");

        // Same inputs should produce same key
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_from_parts_uniqueness() {
        let key1 = CacheKey::from_parts("srv1", "tool", "params");
        let key2 = CacheKey::from_parts("srv2", "tool", "params");
        let key3 = CacheKey::from_parts("srv1", "tool2", "params");
        let key4 = CacheKey::from_parts("srv1", "tool", "params2");

        // Different inputs should produce different keys
        assert_ne!(key1, key2);
        assert_ne!(key1, key3);
        assert_ne!(key1, key4);
    }

    #[test]
    fn test_cache_key_from_str() {
        let key: CacheKey = "test-key".into();
        assert_eq!(key.as_str(), "test-key");

        let key2: CacheKey = String::from("test-key-2").into();
        assert_eq!(key2.as_str(), "test-key-2");
    }

    #[test]
    fn test_cache_key_equality() {
        let key1 = CacheKey::new("key");
        let key2 = CacheKey::new("key");
        let key3 = CacheKey::new("other");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_cache_key_into_inner() {
        let key = CacheKey::new("test");
        assert_eq!(key.into_inner(), "test");
    }

    // Security tests for SessionId
    #[test]
    fn test_session_id_uniqueness() {
        use std::collections::HashSet;
        let mut ids = HashSet::new();

        // Generate 1000 IDs and ensure no collisions
        for _ in 0..1000 {
            let id = SessionId::generate();
            assert!(ids.insert(id), "SessionId collision detected");
        }
    }

    #[test]
    fn test_session_id_unpredictable() {
        let id1 = SessionId::generate();
        let id2 = SessionId::generate();

        // IDs should be completely different (not sequential)
        assert_ne!(id1, id2);

        // Should be random UUIDs with session_ prefix
        assert!(id1.as_str().starts_with("session_"));
        assert!(id2.as_str().starts_with("session_"));

        // UUID format: session_ + 8-4-4-4-12 hex chars with hyphens
        // Total length: 8 (prefix) + 36 (UUID) = 44 characters
        assert_eq!(id1.as_str().len(), 44);
        assert_eq!(id2.as_str().len(), 44);
    }

    // Security tests for CacheKey
    #[test]
    fn test_cache_key_collision_resistance() {
        // These should produce different hashes
        let key1 = CacheKey::from_parts("server", "tool", "params");
        let key2 = CacheKey::from_parts("server::", "tool", "params");
        let key3 = CacheKey::from_parts("server", "tool::", "params");
        let key4 = CacheKey::from_parts("serv", "er::tool", "params");

        assert_ne!(key1, key2);
        assert_ne!(key1, key3);
        assert_ne!(key1, key4);
        assert_ne!(key2, key3);
        assert_ne!(key3, key4);
    }

    #[test]
    fn test_cache_key_deterministic() {
        // Same inputs should produce same hash
        let key1 = CacheKey::from_parts("server", "tool", "params");
        let key2 = CacheKey::from_parts("server", "tool", "params");

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_key_null_byte_separation() {
        // Null byte separator should prevent these from colliding
        let key1 = CacheKey::from_parts("ab", "cd", "ef");
        let key2 = CacheKey::from_parts("a", "bcd", "ef");
        let key3 = CacheKey::from_parts("abc", "d", "ef");
        let key4 = CacheKey::from_parts("ab", "c", "def");

        // All should be different due to null byte separators
        assert_ne!(key1, key2);
        assert_ne!(key1, key3);
        assert_ne!(key1, key4);
        assert_ne!(key2, key3);
        assert_ne!(key2, key4);
        assert_ne!(key3, key4);
    }

    // ========================================================================
    // SkillDescription validation tests
    // ========================================================================

    #[test]
    fn test_skill_description_valid() {
        let desc = SkillDescription::new(
            "Sends messages to VK Teams. Use when working with VK messenger.",
        )
        .unwrap();
        assert_eq!(
            desc.as_str(),
            "Sends messages to VK Teams. Use when working with VK messenger."
        );
    }

    #[test]
    fn test_skill_description_max_length() {
        // Exactly at max length should succeed
        let max_desc = "x".repeat(SkillDescription::MAX_LENGTH);
        assert!(SkillDescription::new(&max_desc).is_ok());
    }

    #[test]
    fn test_skill_description_too_long() {
        // One char over max length should fail
        let too_long = "x".repeat(SkillDescription::MAX_LENGTH + 1);
        let result = SkillDescription::new(&too_long);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_empty() {
        let result = SkillDescription::new("");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_xml_tags_less_than() {
        let result = SkillDescription::new("Bad <script> tag");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_xml_tags_greater_than() {
        let result = SkillDescription::new("Bad tag >");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_xml_tags_both() {
        let result = SkillDescription::new("<script>alert(1)</script>");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_template_syntax_opening() {
        let result = SkillDescription::new("Bad {{injection");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_template_syntax_closing() {
        let result = SkillDescription::new("Bad injection}}");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_template_syntax_both() {
        let result = SkillDescription::new("Bad {{injection}}");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_description_multiline() {
        let desc = SkillDescription::new(
            "Interact with VK Teams messenger to send messages.\n\
             Use when working with VK Teams or when user mentions VK messenger.",
        )
        .unwrap();
        assert!(desc.as_str().contains('\n'));
    }

    #[test]
    fn test_skill_description_unicode() {
        let desc = SkillDescription::new("Sends messages with emoji 🚀 and unicode ñ.").unwrap();
        assert!(desc.as_str().contains('🚀'));
    }

    #[test]
    fn test_skill_description_special_chars() {
        // These should be allowed
        let desc =
            SkillDescription::new("Uses @mentions, #tags, $variables, and !commands.").unwrap();
        assert!(desc.as_str().contains('@'));
    }

    #[test]
    fn test_skill_description_display() {
        let desc = SkillDescription::new("Test description").unwrap();
        assert_eq!(format!("{desc}"), "Test description");
    }

    #[test]
    fn test_skill_description_into_inner() {
        let desc = SkillDescription::new("Test").unwrap();
        let inner: String = desc.into_inner();
        assert_eq!(inner, "Test");
    }

    #[test]
    fn test_skill_description_try_from_string() {
        use std::convert::TryFrom;

        let result = SkillDescription::try_from(String::from("Valid description"));
        assert!(result.is_ok());

        let result = SkillDescription::try_from(String::from("<invalid>"));
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_description_try_from_str() {
        use std::convert::TryFrom;

        let result = SkillDescription::try_from("Valid description");
        assert!(result.is_ok());

        let result = SkillDescription::try_from("{{invalid}}");
        assert!(result.is_err());
    }

    // ========================================================================
    // SkillName validation tests
    // ========================================================================

    #[test]
    fn test_skill_name_valid_lowercase() {
        assert!(SkillName::new("vkteams-bot").is_ok());
        assert!(SkillName::new("my_skill").is_ok());
        assert!(SkillName::new("skill123").is_ok());
    }

    #[test]
    fn test_skill_name_valid_single_char() {
        assert!(SkillName::new("a").is_ok());
        assert!(SkillName::new("z").is_ok());
    }

    #[test]
    fn test_skill_name_valid_max_length() {
        let max_name = "a".to_string() + &"x".repeat(SkillName::MAX_LENGTH - 1);
        assert!(SkillName::new(&max_name).is_ok());
    }

    #[test]
    fn test_skill_name_valid_with_numbers() {
        assert!(SkillName::new("skill123").is_ok());
        assert!(SkillName::new("a123b456").is_ok());
    }

    #[test]
    fn test_skill_name_valid_with_hyphens() {
        assert!(SkillName::new("my-skill").is_ok());
        assert!(SkillName::new("vk-teams-bot").is_ok());
    }

    #[test]
    fn test_skill_name_valid_with_underscores() {
        assert!(SkillName::new("my_skill").is_ok());
        assert!(SkillName::new("vk_teams_bot").is_ok());
    }

    #[test]
    fn test_skill_name_valid_mixed() {
        assert!(SkillName::new("my_skill-123").is_ok());
        assert!(SkillName::new("vk-teams_bot_v2").is_ok());
    }

    #[test]
    fn test_skill_name_empty() {
        let result = SkillName::new("");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_name_too_long() {
        let too_long = "a".to_string() + &"x".repeat(SkillName::MAX_LENGTH);
        let result = SkillName::new(&too_long);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    #[test]
    fn test_skill_name_reserved_word_anthropic() {
        assert!(SkillName::new("anthropic-skill").is_err());
        assert!(SkillName::new("my-anthropic-bot").is_err());
        assert!(SkillName::new("anthropic").is_err());
    }

    #[test]
    fn test_skill_name_reserved_word_claude() {
        assert!(SkillName::new("claude-bot").is_err());
        assert!(SkillName::new("my-claude-helper").is_err());
        assert!(SkillName::new("claude").is_err());
    }

    #[test]
    fn test_skill_name_reserved_word_case_insensitive() {
        assert!(SkillName::new("ANTHROPIC-skill").is_err());
        assert!(SkillName::new("Claude-bot").is_err());
        assert!(SkillName::new("AnThRoPiC").is_err());
    }

    #[test]
    fn test_skill_name_reserved_word_partial() {
        // Should fail if reserved word appears anywhere
        assert!(SkillName::new("myanthropicbot").is_err());
        assert!(SkillName::new("claudehelper").is_err());
    }

    #[test]
    fn test_skill_name_xml_tags() {
        assert!(SkillName::new("<script>").is_err());
        assert!(SkillName::new("skill<tag>").is_err());
        assert!(SkillName::new("tag>skill").is_err());
    }

    #[test]
    fn test_skill_name_uppercase() {
        assert!(SkillName::new("MySkill").is_err());
        assert!(SkillName::new("SKILL").is_err());
        assert!(SkillName::new("Skill").is_err());
    }

    #[test]
    fn test_skill_name_starts_with_number() {
        assert!(SkillName::new("123skill").is_err());
        assert!(SkillName::new("1skill").is_err());
    }

    #[test]
    fn test_skill_name_starts_with_hyphen() {
        assert!(SkillName::new("-skill").is_err());
    }

    #[test]
    fn test_skill_name_starts_with_underscore() {
        assert!(SkillName::new("_skill").is_err());
    }

    #[test]
    fn test_skill_name_ends_with_hyphen() {
        assert!(SkillName::new("skill-").is_err());
    }

    #[test]
    fn test_skill_name_ends_with_underscore() {
        assert!(SkillName::new("skill_").is_err());
    }

    #[test]
    fn test_skill_name_ends_with_number() {
        // This should be valid
        assert!(SkillName::new("skill123").is_ok());
    }

    #[test]
    fn test_skill_name_special_chars() {
        assert!(SkillName::new("skill@bot").is_err());
        assert!(SkillName::new("skill#tag").is_err());
        assert!(SkillName::new("skill$var").is_err());
        assert!(SkillName::new("skill!cmd").is_err());
        assert!(SkillName::new("skill space").is_err());
    }

    #[test]
    fn test_skill_name_display() {
        let name = SkillName::new("test-skill").unwrap();
        assert_eq!(format!("{name}"), "test-skill");
    }

    #[test]
    fn test_skill_name_into_inner() {
        let name = SkillName::new("test").unwrap();
        let inner: String = name.into_inner();
        assert_eq!(inner, "test");
    }

    #[test]
    fn test_skill_name_try_from_string() {
        use std::convert::TryFrom;

        let result = SkillName::try_from(String::from("valid-skill"));
        assert!(result.is_ok());

        let result = SkillName::try_from(String::from("INVALID"));
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_name_try_from_str() {
        use std::convert::TryFrom;

        let result = SkillName::try_from("valid-skill");
        assert!(result.is_ok());

        let result = SkillName::try_from("anthropic-skill");
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_name_equality() {
        let name1 = SkillName::new("test-skill").unwrap();
        let name2 = SkillName::new("test-skill").unwrap();
        let name3 = SkillName::new("other-skill").unwrap();

        assert_eq!(name1, name2);
        assert_ne!(name1, name3);
    }

    #[test]
    fn test_skill_name_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(SkillName::new("skill1").unwrap());
        set.insert(SkillName::new("skill2").unwrap());
        set.insert(SkillName::new("skill1").unwrap()); // Duplicate

        assert_eq!(set.len(), 2);
    }

    // ========================================================================
    // Error detection tests
    // ========================================================================

    #[test]
    fn test_validation_error_detection() {
        let err = SkillName::new("").unwrap_err();
        assert!(err.is_validation_error());
        assert!(!err.is_reserved_word_error());
    }

    #[test]
    fn test_reserved_word_error_detection() {
        let err = SkillName::new("anthropic-skill").unwrap_err();
        assert!(err.is_reserved_word_error());
        assert!(!err.is_validation_error());
    }

    #[test]
    fn test_error_display_validation() {
        let err = SkillName::new("").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Validation error"));
        assert!(msg.contains("skill_name"));
    }

    #[test]
    fn test_error_display_reserved_word() {
        let err = SkillName::new("claude-bot").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Reserved word"));
        assert!(msg.contains("claude"));
    }

    // ========================================================================
    // Edge case and security tests
    // ========================================================================

    #[test]
    fn test_skill_description_very_long() {
        let desc = "x".repeat(10_000);
        let result = SkillDescription::new(&desc);
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_description_null_bytes() {
        // Null bytes should be allowed (no special handling)
        let desc = SkillDescription::new("Test\0description").unwrap();
        assert!(desc.as_str().contains('\0'));
    }

    #[test]
    fn test_skill_name_unicode() {
        // Unicode letters are not allowed (only ASCII lowercase)
        assert!(SkillName::new("café").is_err());
        assert!(SkillName::new("über").is_err());
        assert!(SkillName::new("日本語").is_err());
    }

    #[test]
    fn test_skill_name_boundaries() {
        // Test exact length boundaries
        assert!(SkillName::new("a").is_ok()); // Exactly 1
        assert!(SkillName::new(&("a".to_string() + &"b".repeat(63))).is_ok()); // Exactly 64
        assert!(SkillName::new(&("a".to_string() + &"b".repeat(64))).is_err()); // 65
    }

    #[test]
    fn test_skill_description_boundaries() {
        let at_max = "x".repeat(1024);
        let over_max = "x".repeat(1025);

        assert!(SkillDescription::new(&at_max).is_ok());
        assert!(SkillDescription::new(&over_max).is_err());
    }

    #[test]
    fn test_reserved_word_substring() {
        // "anthropic" appears as substring
        assert!(SkillName::new("myanthropicbot").is_err());
        assert!(SkillName::new("anthropichelper").is_err());
        assert!(SkillName::new("theanthropic").is_err());
    }

    #[test]
    fn test_similar_to_reserved_words() {
        // These should be allowed (not exact matches)
        assert!(SkillName::new("anthro").is_ok());
        assert!(SkillName::new("claud").is_ok());
        assert!(SkillName::new("anthr0pic").is_ok()); // With zero
    }
}
