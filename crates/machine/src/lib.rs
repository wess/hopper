//! Native VM execution, independent of the container engine and UI.

pub mod arm;
pub mod debug;
pub mod acpi;
pub mod devices;
pub mod dma;
pub mod platform;
pub mod psci;
pub mod smccc;
pub mod setup;
pub mod runtime;
pub mod pause;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod hypervisor;
