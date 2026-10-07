//! The async service facade the UI calls.
//!
//! `Host` owns the Docker client and the engine status, and exposes one method
//! per user-facing operation. It is gpui-free: the app reaches it through the
//! tokio bridge, which is the only seam between the async world and the
//! renderer.

#[cfg(target_os = "macos")]
pub mod appleinstall;
pub mod engine;
pub mod facade;
pub mod import;
pub mod machines;
pub mod registry;
pub mod runtime;
pub mod stacks;
pub mod status;

pub use facade::Host;

pub use ::engine::machines::{Actor as MachineActor, Machines};
#[cfg(unix)]
pub use ::engine::machines::native::sessions::remote::Lifecycle as MachineLifecycle;
/// Re-export the interactive exec session so the UI can hold one without
/// depending on the docker crate directly.
pub use docker::exec as docker_exec;

pub use ::engine::machines::native::{deployment::Phase as MachinePhase, Frame as MachineFrame, State as MachineState};

pub use ::engine::machines::native::sessions::input::Input as MachineInputLease;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use ::engine::machines::vz::{Action as VirtualMachineAction, Owner as VirtualMachineOwner};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use ::engine::machines::vz::{
  MainThreadMarker as VirtualMachineThread, Prepared as VirtualLinuxPrepared,
  Stage as VirtualLinuxStage,
};
