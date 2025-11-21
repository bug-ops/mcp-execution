//! MCP integration examples and testing infrastructure.
#![allow(clippy::format_push_string)]
#![allow(clippy::unused_async)]
#![allow(clippy::cast_possible_truncation)] // u128->u64 for millis is safe in practice
#![allow(clippy::cast_precision_loss)] // usize->f64 for statistics is acceptable
#![allow(clippy::too_many_lines)] // Test/example code can be longer
#![allow(clippy::unreadable_literal)] // Test data can have raw numbers
#![allow(clippy::float_cmp)] // Exact comparisons needed for test assertions
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
