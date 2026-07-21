//! Tool adapters — V1.1 §6.
//!
//! W3a: FilesystemTool (native Rust, no MCP SDK yet).
//! W3b/W4 will wrap this in an MCP server handler.

pub mod fs_paths;
pub mod fs_snapshot;
pub mod fs;
pub mod diff;
