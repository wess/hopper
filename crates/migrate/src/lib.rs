//! Docker Desktop → Hopper migration.

#[cfg(target_os = "macos")]
pub mod apple;
pub mod run;
pub mod scan;

pub use scan::scan;
