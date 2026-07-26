//! MCP server handler skeleton — V1.1 §6.1 + Appendix B.
//!
//! W3b: schema + dispatch. No JSON-RPC transport — that's W4.
//! W7 Plan 5 Task 0: `client` module added for spawning external MCP servers
//! (e.g. Playwright MCP) and invoking their tools over stdio JSON-RPC.

pub mod client;
pub mod handler;
pub mod repo;
pub mod schema;
pub mod server;
pub mod transport;
