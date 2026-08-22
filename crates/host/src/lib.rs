//! The async service facade the UI calls.
//!
//! `Host` owns the Docker client and the engine status, and exposes one method
//! per user-facing operation. It is gpui-free: the app reaches it through the
//! tokio bridge, which is the only seam between the async world and the
//! renderer.

#[cfg(target_os = "macos")]
pub mod appleinstall;
pub mod compose;
pub mod engine;
pub mod runtime;
pub mod facade;
pub mod import;
pub mod registry;
pub mod status;

pub use facade::Host;

/// Re-export the interactive exec session so the UI can hold one without
/// depending on the docker crate directly.
pub use docker::exec as docker_exec;
