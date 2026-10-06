//! The providers Hopper can attach to or supply.

#[cfg(target_os = "macos")]
pub mod apple;
pub mod existing;
pub mod linux;
pub mod named;
pub mod vm;

#[cfg(target_os = "macos")]
pub use apple::AppleContainers;
pub use existing::Existing;
pub use linux::Linux;
pub use named::Named;
