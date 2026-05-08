//! codeterm-mem-store: typed graph mutation API.
//! Both the Tauri daemon and the CLI worker depend on this crate so all
//! mutations go through one set of typed functions and one set of tests.

pub mod audit;
pub mod cluster;
pub mod error;
pub mod graph;
pub mod migrations;
pub mod plan_format;

pub use error::{MemStoreError, MemStoreResult};

#[cfg(test)]
mod tests;
