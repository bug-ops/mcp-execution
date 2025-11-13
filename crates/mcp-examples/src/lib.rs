//! MCP integration examples and testing infrastructure.
//!
//! This crate contains example programs demonstrating the functionality
//! of the MCP Code Execution framework, along with utilities for testing,
//! metrics collection, and token analysis.
//!
//! # Modules
//!
//! - [`mock_server`] - Mock MCP server for testing
//! - [`metrics`] - Performance metrics collection
//! - [`token_analysis`] - Token usage analysis and savings calculation

pub mod metrics;
pub mod mock_server;
pub mod token_analysis;
