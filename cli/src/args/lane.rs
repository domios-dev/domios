use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum LaneAction {
    /// Run the daemon-owned diagnostics check for a task or lane branch
    Check(LaneSubjectArgs),
    /// Read the stored diagnostics for a task
    Diagnostics(LaneSubjectArgs),
}

#[derive(Args)]
pub struct LaneSubjectArgs {
    /// Task id, task handle, or recorded lane branch
    pub subject: String,
    /// Plan id (default: $CODETERM_PLAN_ID)
    #[arg(long, env = "CODETERM_PLAN_ID")]
    pub plan: Option<String>,
}
