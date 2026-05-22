//! Per-pane JSONL chat tailers — UI-mode streaming pipeline.
//!
//! When a user flips a pane into UI mode the API caller starts a
//! `Tailer` keyed on `pane_id`. The tailer polls the session JSONL
//! through the provider plugin and emits delta messages via a
//! [`DeltaSink`]. The tailer keeps a byte-offset bookmark so each
//! poll only carries new content.
//!
//! All provider-specific logic (where the JSONL lives, how to parse
//! it) lives in plugins — this module only orchestrates
//! start/stop and shuttles the bytes through.

pub mod raw_text_sink;
pub mod registry;
pub mod tailer;
pub mod tauri_sink;

pub use registry::ChatRegistry;
pub use tailer::{
    spawn_raw_tailer, spawn_tailer, DeltaSink, RawTailConfig, RawTextSink, TailConfig,
    TailerHandle,
};

#[cfg(test)]
mod tailer_tests;
