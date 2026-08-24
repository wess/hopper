//! Apple Containers as a Hopper backend.
//!
//! On macOS 26 Apple ships its own container runtime: each container is a
//! lightweight VM, images are OCI, and the whole thing is maintained by the
//! platform vendor. It is what Hopper's own VM used to be for, done natively.
//!
//! The catch is the interface. `container` talks to `container-apiserver` over
//! XPC and exposes no Docker Engine API — a request to publish one was closed
//! as not planned — so this crate drives the CLI and maps its JSON onto the
//! same `model` types the Engine API path produces. Views never learn which
//! backend answered.
//!
//! What Apple's runtime does not do, and callers should not offer:
//!
//! - pause / unpause, rename, and container resource updates after creation
//! - restart policies and `--hostname`
//! - an event stream (Hopper polls instead)
//! - healthchecks, so every container reports `Health::None`
//! - Compose. Apple ships none, and there is no Docker socket for the real
//!   one to reach — so Hopper reads the compose file and runs the services
//!   itself, through `container run`. See the `compose` crate.

pub mod cli;
pub mod containers;
pub mod images;
pub mod networks;
pub mod system;
pub mod volumes;
pub mod wire;

pub use cli::Cli;

/// Is Apple's runtime installed on this machine?
pub fn installed() -> bool {
    Cli::locate().is_some()
}

/// Where the signed installer can be fetched, for the first-run panel.
pub const INSTALLER_URL: &str =
    "https://github.com/apple/container/releases/latest/download/container-installer-signed.pkg";

/// The project page, for a user who would rather read before installing.
pub const HOMEPAGE: &str = "https://github.com/apple/container";
