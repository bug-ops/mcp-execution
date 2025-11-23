---
applyTo: "crates/mcp-introspector/**/*.rs"
---
# Copilot Instructions: mcp-introspector

This crate provides **MCP server introspection** using the **official rmcp SDK**. It discovers server capabilities, tools, resources, and prompts for code generation.

## Error Handling

**Use `thiserror`** for all errors:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IntrospectorError {
    #[error("failed to connect to server '{server}'")]
    ConnectionFailed {
        server: String,
        #[source]
        source: std::io::Error,
    },

    #[error("server '{server}' has no tools")]
    NoToolsAvailable { server: String },

    #[error("rmcp error: {0}")]
    RmcpError(#[from] rmcp::Error),

    #[error("invalid server response: {0}")]
    InvalidResponse(String),
}

pub type Result<T> = std::result::Result<T, IntrospectorError>;
```

## rmcp SDK Integration - CRITICAL

**ALWAYS use the official rmcp SDK**:

### ServiceExt Trait

The `ServiceExt` trait provides high-level MCP operations:

```rust
use rmcp::ServiceExt;
use rmcp::transport::{TokioChildProcess, ConfigureCommandExt};
use rmcp::client::Client;

// ✅ GOOD: Use ServiceExt methods
async fn discover_server(command: &str) -> Result<ServerInfo> {
    // Create transport
    let mut cmd = std::process::Command::new(command);
    cmd.stdin(std::process::Stdio::piped())
       .stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::inherit());

    let transport = TokioChildProcess::new(cmd)
        .await
        .map_err(|e| IntrospectorError::ConnectionFailed {
            server: command.to_string(),
            source: e.into(),
        })?;

    let client = Client::new(transport);

    // Get server info
    let info = client.get_server_info().await?;

    // Convert to our types
    Ok(ServerInfo {
        name: info.name,
        version: info.version,
        capabilities: convert_capabilities(info.capabilities),
    })
}
```

### Discovering Tools

```rust
use rmcp::ServiceExt;

async fn list_tools(client: &Client<TokioChildProcess>) -> Result<Vec<ToolInfo>> {
    // List all available tools
    let tools_response = client.list_tools().await?;

    let mut tools = Vec::new();

    for tool in tools_response.tools {
        tools.push(ToolInfo {
            name: tool.name,
            description: tool.description.unwrap_or_default(),
            input_schema: tool.input_schema,
        });
    }

    Ok(tools)
}
```

### Discovering Resources

```rust
use rmcp::ServiceExt;

async fn list_resources(client: &Client<TokioChildProcess>) -> Result<Vec<ResourceInfo>> {
    // Check if resources are supported
    let info = client.get_server_info().await?;

    if !info.capabilities.resources.map(|r| r.subscribe).unwrap_or(false) {
        return Ok(Vec::new());
    }

    // List resources
    let resources_response = client.list_resources().await?;

    let mut resources = Vec::new();

    for resource in resources_response.resources {
        resources.push(ResourceInfo {
            uri: resource.uri,
            name: resource.name,
            description: resource.description,
            mime_type: resource.mime_type,
        });
    }

    Ok(resources)
}
```

### Discovering Prompts

```rust
use rmcp::ServiceExt;

async fn list_prompts(client: &Client<TokioChildProcess>) -> Result<Vec<PromptInfo>> {
    let prompts_response = client.list_prompts().await?;

    let mut prompts = Vec::new();

    for prompt in prompts_response.prompts {
        prompts.push(PromptInfo {
            name: prompt.name,
            description: prompt.description,
            arguments: prompt.arguments.unwrap_or_default(),
        });
    }

    Ok(prompts)
}
```

## Introspector API

The main introspector struct manages discovery:

```rust
use std::collections::HashMap;
use mcp_core::ServerId;

pub struct Introspector {
    // Cache discovered servers
    servers: HashMap<ServerId, ServerInfo>,
}

impl Introspector {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Discover a server's capabilities and tools.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_introspector::Introspector;
    /// # use mcp_core::ServerId;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    ///
    /// let server_id = ServerId::new("github");
    /// let info = introspector
    ///     .discover_server(server_id, "github-server")
    ///     .await?;
    ///
    /// println!("Found {} tools", info.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn discover_server(
        &mut self,
        server_id: ServerId,
        command: &str,
    ) -> Result<ServerInfo> {
        // Check cache
        if let Some(cached) = self.servers.get(&server_id) {
            return Ok(cached.clone());
        }

        // Connect to server
        let mut cmd = std::process::Command::new(command);
        cmd.stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::inherit());

        let transport = TokioChildProcess::new(cmd)
            .await
            .map_err(|e| IntrospectorError::ConnectionFailed {
                server: command.to_string(),
                source: e.into(),
            })?;

        let client = Client::new(transport);

        // Get server info
        let server_info = client.get_server_info().await?;

        // Discover tools
        let tools = if server_info.capabilities.tools.is_some() {
            list_tools(&client).await?
        } else {
            Vec::new()
        };

        // Discover resources
        let resources = if server_info.capabilities.resources.is_some() {
            list_resources(&client).await?
        } else {
            Vec::new()
        };

        // Discover prompts
        let prompts = if server_info.capabilities.prompts.is_some() {
            list_prompts(&client).await?
        } else {
            Vec::new()
        };

        let info = ServerInfo {
            name: server_info.name,
            version: server_info.version,
            tools,
            resources,
            prompts,
        };

        // Cache result
        self.servers.insert(server_id, info.clone());

        Ok(info)
    }

    /// Get cached server info if available.
    pub fn get_cached(&self, server_id: &ServerId) -> Option<&ServerInfo> {
        self.servers.get(server_id)
    }
}
```

## Schema Conversion

Convert rmcp schemas to our internal format:

```rust
use serde_json::Value;

fn convert_input_schema(schema: Value) -> Schema {
    // rmcp uses JSON Schema format
    // Extract properties and required fields

    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|p| {
            p.iter()
                .map(|(name, prop)| {
                    let param_type = prop
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("string");

                    SchemaProperty {
                        name: name.clone(),
                        type_: param_type.to_string(),
                        description: prop
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from),
                        required: false,  // Set below
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Mark required properties
    let properties = properties
        .into_iter()
        .map(|mut prop| {
            prop.required = required.contains(&prop.name);
            prop
        })
        .collect();

    Schema { properties }
}
```

## Timeout Handling

Always use timeouts when connecting to servers:

```rust
use tokio::time::timeout;

impl Introspector {
    pub async fn discover_server_with_timeout(
        &mut self,
        server_id: ServerId,
        command: &str,
        timeout_duration: Duration,
    ) -> Result<ServerInfo> {
        timeout(timeout_duration, self.discover_server(server_id, command))
            .await
            .map_err(|_| IntrospectorError::Timeout {
                timeout: timeout_duration,
            })?
    }
}
```

## Testing

Test with mock MCP servers:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_tools() {
        // Use a test MCP server that returns known tools
        let mut introspector = Introspector::new();

        let server_id = ServerId::new("test-server");
        let info = introspector
            .discover_server(server_id, "test-mcp-server")
            .await
            .unwrap();

        assert!(!info.tools.is_empty());
        assert_eq!(info.name, "test-server");
    }

    #[tokio::test]
    async fn test_caching() {
        let mut introspector = Introspector::new();

        let server_id = ServerId::new("test-server");

        // First call - discovers
        let info1 = introspector
            .discover_server(server_id.clone(), "test-mcp-server")
            .await
            .unwrap();

        // Second call - from cache
        let info2 = introspector.get_cached(&server_id).unwrap();

        assert_eq!(info1.name, info2.name);
    }
}
```

## Summary

- **Always use rmcp SDK** with `ServiceExt` trait
- **Use `thiserror`** for all errors
- **Cache discovered servers** to avoid repeated introspection
- **Convert rmcp types** to internal types appropriately
- **Handle timeouts** for network operations
- **Test with mock servers** to avoid external dependencies
