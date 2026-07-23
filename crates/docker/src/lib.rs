//! The Docker Engine API client and every domain module built on it.
//!
//! This crate is gpui-free: it knows about the daemon and nothing about the
//! UI. The layer above (`host`) composes it into the service facade the app
//! calls.

pub mod archive;
pub mod build;
pub mod client;
pub mod compose;
pub mod containers;
pub mod credentials;
pub mod exec;
pub mod images;
pub mod logs;
pub mod networks;
pub mod stats;
pub mod system;
pub mod volumes;
pub mod demux;
pub mod endpoint;
pub mod error;
pub mod transport;

pub use client::{Client, Req};
pub use endpoint::Endpoint;
pub use error::{DockerError, ErrorKind, Result};
