//! Yinkote plugin runtime.
//!
//! ## Contract
//!
//! A plugin is a directory containing `plugin.json`:
//!
//! ```json
//! {
//!   "id": "crossref",
//!   "name": "Crossref",
//!   "version": "1.0.0",
//!   "apiVersion": 1,
//!   "runtime": { "type": "process", "command": "node", "args": ["main.js"] },
//!   "capabilities": ["metadata_source"],
//!   "permissions": ["network", "items_read"],
//!   "hooks": ["item.created"]
//! }
//! ```
//!
//! The process speaks newline-delimited JSON-RPC 2.0 on stdio, in both
//! directions: the host calls `initialize` / `hook` / custom methods, and the
//! plugin may call back into `host.*` methods subject to its declared
//! permissions.

pub mod host;
pub mod manifest;
pub mod process;
pub mod rpc;

pub use host::{BuiltinPlugin, PluginHostBuilder, PluginRegistry};
pub use manifest::{discover, validate, Discovered};
pub use process::PluginProcess;
