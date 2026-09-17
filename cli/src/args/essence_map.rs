use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum EssenceMapAction {
    /// Generate essences/map.jsonl from the capability and core essences.
    Generate(EssenceMapPaths),
    /// Validate a generated essence map and its append-only decisions log.
    Validate(EssenceMapPaths),
    /// Append one decision explaining the current map to the decisions log.
    Record(EssenceMapRecord),
    /// Summarize the map decisions of a release and offer the revert choice.
    ReleaseSummary(EssenceMapReleaseSummary),
    /// Restore the map recorded by an earlier decision and log the revert.
    Revert(EssenceMapRevert),
}

#[derive(Args, Clone)]
pub struct EssenceMapPaths {
    /// Repository root containing the essences directory.
    #[arg(long, default_value = ".", value_name = "PATH")]
    pub root: PathBuf,

    /// Map JSONL path, relative to --root unless absolute.
    #[arg(long, value_name = "PATH")]
    pub map: Option<PathBuf>,

    /// Decisions JSONL path, relative to --root unless absolute.
    #[arg(long, value_name = "PATH")]
    pub decisions: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub struct EssenceMapRecord {
    #[command(flatten)]
    pub paths: EssenceMapPaths,

    /// What changed in the map.
    #[arg(long, value_name = "TEXT")]
    pub summary: String,

    /// Why the change is correct against the source essences.
    #[arg(long, value_name = "TEXT")]
    pub reason: String,
}

#[derive(Args, Clone)]
pub struct EssenceMapReleaseSummary {
    #[command(flatten)]
    pub paths: EssenceMapPaths,

    /// Baseline decision id; the summary covers every decision after it.
    #[arg(long, value_name = "DECISION_ID")]
    pub since: Option<String>,
}

#[derive(Args, Clone)]
pub struct EssenceMapRevert {
    #[command(flatten)]
    pub paths: EssenceMapPaths,

    /// Decision id whose recorded map is restored.
    #[arg(long, value_name = "DECISION_ID")]
    pub to: String,
}
