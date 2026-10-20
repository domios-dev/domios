//! Orchestration watcher: observation snapshots and verdict guardrails.

pub mod observe;
pub mod verdict;

pub use observe::{
    build_observation, collect_watch_group, observation_config_from_json, ObservationConfig,
    ObservationSources, WatchGroup,
};
pub use verdict::{
    apply_guardrails, parse_verdict, VerdictOutcome, WatcherAction, WatcherVerdict,
    WatcherVerdictStatus, STATE_BLOB_MAX_BYTES,
};
