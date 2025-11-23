# ADR-003: Strong Types Over Primitives

## Status

Accepted

## Context

The project uses many domain-specific identifiers and values (server IDs, tool names, memory limits) that could be represented as primitive types (`String`, `usize`). Microsoft Rust Guidelines strongly recommend "appropriate std types" and avoiding primitive obsession.

## Decision

We will use **newtype wrappers** for all domain-specific values:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolName(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLimit(usize);
```

Each type provides:

- Constructors with validation
- Accessor methods (e.g., `as_str()`, `bytes()`)
- Appropriate trait implementations (`Debug`, `Clone`, `Hash`, `Eq`)

## Rationale

**Type safety benefits:**

1. **Prevents mixing**: Cannot accidentally pass `ServerId` where `ToolName` expected
2. **Self-documenting**: API signatures are clear: `fn call_tool(server: &ServerId, tool: &ToolName)`
3. **Validation centralization**: Validation logic in constructor, not scattered
4. **Refactoring safety**: Changing internal representation doesn't break API

**Example prevented error:**

```rust
// With primitives (BAD):
fn call_tool(server: &str, tool: &str) -> Result<Value>;
call_tool("my-tool", "my-server");  // Arguments swapped, compiles!

// With strong types (GOOD):
fn call_tool(server: &ServerId, tool: &ToolName) -> Result<Value>;
call_tool(&tool_name, &server_id);  // Compiler error!
```

## Consequences

**Positive:**

- Compiler enforces correct usage
- Clear intent in function signatures
- Centralized validation and constraints
- Easy to add behavior to types later
- Follows Microsoft Rust Guidelines

**Negative:**

- More boilerplate (type definitions)
- Need to call constructors/accessors
- Slight runtime overhead (usually optimized away)

**Mitigation:**

- Implement `From<T>` for ergonomic conversion
- Implement `AsRef<str>` for string types
- Use `#[inline]` for accessors
- Provide builder patterns where appropriate

## Implementation Pattern

```rust
impl ServerId {
    /// Create a new server identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// let id = ServerId::new("github");
    /// ```
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the server ID as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ServerId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
```

## References

- Microsoft Rust Guidelines: "Use the appropriate std type"
- Domain-Driven Design: Value Objects pattern
- Rust API Guidelines: Newtype pattern (C-NEWTYPE)
