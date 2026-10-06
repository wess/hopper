//! Native VM execution, independent of the container engine and UI.

pub mod arm;
pub mod devices;
pub mod platform;
pub mod psci;
pub mod smccc;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod hypervisor;
