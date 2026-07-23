//! Per-platform provider implementations.

pub mod existing;

#[cfg(target_os = "linux")]
pub mod linux;

pub use existing::Existing;
