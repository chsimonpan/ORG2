//! Anthropic Messages API client.
//!
//! Speaks the native Anthropic Messages API format (`/v1/messages`),
//! which is different from the OpenAI chat completions format.
//!
//! Used for direct Anthropic API access and any gateway that only
//! supports the Anthropic format (not the OpenAI-compatible layer).

pub mod client;
mod errors;
mod messages;
mod request;
mod stream_parser;
#[cfg(test)]
mod stream_parser_tests;
pub mod streaming;
mod thinking;
mod tools;
#[cfg(test)]
#[path = "tests/task_fork_prefix_tests.rs"]
mod task_fork_prefix_tests;
pub mod types;
mod usage;

pub use client::{AnthropicAuthMode, AnthropicClient, ClaudeOAuthRefreshConfig};
