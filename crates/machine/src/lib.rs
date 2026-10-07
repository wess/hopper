//! Native VM execution, independent of the container engine and UI.

pub mod acpi;
pub mod arm;
pub mod debug;
pub mod devices;
pub mod dma;
pub mod ipc;
pub mod pause;
pub mod platform;
pub mod psci;
pub mod runtime;
pub mod setup;
pub mod smccc;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod hypervisor;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod vz;
