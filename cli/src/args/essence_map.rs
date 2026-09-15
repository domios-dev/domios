use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum EssenceMapAction {
    /// Generate essences/map.jsonl from the capability and core essences.
    Generate(EssenceMapPaths),
    /// Validate a generated essence map and its append-only decisions log.
    Validate(EssenceMapPaths),
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
