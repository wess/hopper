//! Compose stacks: recognising them, and running them.
//!
//! Two halves that meet at the labels. [`group`] reads
//! `com.docker.compose.*` off containers that exist and reconstructs the
//! stacks they belong to; [`run`] takes a plan and creates containers wearing
//! exactly those labels. Neither knows which engine answered.

pub mod group;
pub mod run;

pub use group::{can_start_from_files, group};
