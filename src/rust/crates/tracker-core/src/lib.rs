//! tracker-core: Core types and IPC protocol for Agent Tracker
//!
//! This crate defines the shared types used by tracker-server, tracker-tui, and tracker-web.

pub mod ipc;
pub mod types;

pub use ipc::*;
pub use types::*;
