//! The macOS managed engine.
//!
//! Hopper runs a minimal Linux guest on Apple's Virtualization framework and
//! talks to the `dockerd` inside it. In the Bun build this lived in a separate
//! Swift sidecar (`hopperd`) because the app process carried JIT entitlements
//! that cannot coexist with `com.apple.security.virtualization`. A Rust app has
//! no JIT, so the VM is created in-process and the sidecar disappears — along
//! with its inside-out signing dance.

pub mod forward;
pub mod shares;
pub mod vm;
pub mod machine;
pub mod bridge;
pub mod forwarder;
pub mod provider;
pub mod acquire;
