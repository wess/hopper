//! Hopper's domain types — the shapes every other crate speaks.
//!
//! These are lean projections of the Docker Engine API: enough to drive the UI,
//! not the full inspect dumps (those travel as [`InspectResult`], an opaque
//! JSON record rendered generically).
//!
//! Serialization stays `camelCase` throughout. Nothing in the app depends on
//! that anymore now that both sides are Rust, but the MCP server hands these
//! straight to AI clients as JSON, and `~/.hopper/` files written by the
//! earlier TypeScript build must keep round-tripping.

pub mod compose;
pub mod container;
pub mod engine;
pub mod image;
pub mod migration;
pub mod network;
pub mod registry;
pub mod settings;
pub mod stream;
pub mod system;
pub mod volume;
pub mod workspace;

pub use compose::*;
pub use container::*;
pub use engine::*;
pub use image::*;
pub use migration::*;
pub use network::*;
pub use registry::*;
pub use settings::*;
pub use stream::*;
pub use system::*;
pub use volume::*;
pub use workspace::*;

/// A raw `docker inspect` payload, passed through untyped and rendered as a
/// JSON tree. Typing these would mean tracking every Engine API field across
/// versions for no gain — the UI shows them generically.
pub type InspectResult = serde_json::Value;

/// A fresh identifier for request/session correlation and stored records.
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
