use clap::Subcommand;

#[derive(Subcommand)]
#[allow(
    clippy::large_enum_variant,
    reason = "Clap command enums keep parsed options inline."
)]
pub enum InboxAction {
    /// Add a standalone task to the workspace Inbox
    Add {
        #[arg(long, allow_hyphen_values = true)]
        name: String,
        #[arg(long = "type", allow_hyphen_values = true)]
        kind: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        model: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        why: Option<String>,
        #[arg(long, value_name = "PATH")]
        why_file: Option<String>,
        #[arg(long = "criterion", allow_hyphen_values = true)]
        criterion: Vec<String>,
        #[arg(long, allow_hyphen_values = true)]
        test_plan: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        qa_plan: Option<String>,
        #[arg(long, value_name = "PATH")]
        qa_plan_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        correctness: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        body: Option<String>,
        #[arg(long, value_name = "PATH")]
        body_file: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        intent: Option<String>,
        #[arg(long, value_name = "PATH")]
        intent_file: Option<String>,
        #[arg(long, value_parser = ["repository_mutation", "non_repository"])]
        execution_intent: Option<String>,
        #[arg(long = "assign-tab", value_name = "TAB")]
        assign_tab: Option<String>,
    },
}
