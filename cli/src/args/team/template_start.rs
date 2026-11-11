use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum TemplateAction {
    /// List builtin and saved team templates
    List,
    /// Show what a template starts, asks for and brings
    Preview {
        /// Template name (`name[@version]`)
        name: String,
    },
    /// Start a Team from a template on the standard pair primitive
    Start(TemplateStartArgs),
    /// Send a template to the `--mesh` peer Main's My templates, unverified there
    Transfer {
        /// Template name (`name[@version]`)
        name: String,
        /// The peer tab that receives the package over the mesh attachment rail
        #[arg(long)]
        tab: String,
    },
    /// Read the outcome of a template start asked of the `--mesh` peer Main
    Outcome {
        /// The request id `team template start --mesh` returned
        request: String,
    },
}

#[derive(Args)]
pub struct TemplateStartArgs {
    /// Template name (`name[@version]`)
    pub name: String,
    #[arg(long, env = "CODETERM_WORKSPACE_ID")]
    pub workspace: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub title: Option<String>,
    /// Orchestrator provider, optionally with a model (`provider[:model]`); omitted asks for a choice
    #[arg(long)]
    pub orchestrator: Option<String>,
    /// Planner provider, optionally with a model (`provider[:model]`); omitted asks for a choice
    #[arg(long)]
    pub planner: Option<String>,
    /// Another with_team manager's model as `slot=harness:model`; repeatable
    #[arg(long = "manager-model", value_name = "SLOT=HARNESS:MODEL")]
    pub manager_models: Vec<String>,
    /// A template input as `key=value`; repeatable
    #[arg(long = "input", value_name = "KEY=VALUE", allow_hyphen_values = true)]
    pub inputs: Vec<String>,
    /// A secret binding as `NAME=vault-entry`; repeatable
    #[arg(long = "secret", value_name = "NAME=ENTRY")]
    pub secrets: Vec<String>,
    /// A payload item id to leave out; repeatable
    #[arg(long = "opt-out", value_name = "ITEM")]
    pub opt_out: Vec<String>,
    /// The consent decision id `team template preview` issued
    #[arg(long)]
    pub decision: Option<String>,
    /// Re-run the start on this existing Team: adds only missing registrations
    #[arg(long)]
    pub team: Option<String>,
    /// Explicit leaf declaration, as for `team pair`
    #[arg(long)]
    pub acknowledge_leaf: bool,
}
