use clap::Args;

const FORMATS: [&str; 3] = ["yaml", "team", "folder"];

#[derive(Args)]
pub struct DistillArgs {
    /// Team ID
    pub team_id: String,
    /// Package name, `owner/slug`
    #[arg(long)]
    pub name: String,
    /// Package version (semver)
    #[arg(long, default_value = "1.0.0")]
    pub version: String,
    /// What the Team does, in one sentence
    #[arg(long)]
    pub description: String,
    /// Display title
    #[arg(long)]
    pub title: Option<String>,
    /// SPDX license id
    #[arg(long)]
    pub license: Option<String>,
    /// A situation the template fits; repeat for several
    #[arg(long = "when")]
    pub when: Vec<String>,
    /// File with the runbook the coordinator wrote; it joins the orchestrator's instructions
    #[arg(long)]
    pub runbook_file: Option<String>,
    /// Payload item id to leave out (from the Includes list); repeat for several
    #[arg(long = "opt-out")]
    pub opt_out: Vec<String>,
    /// Package form: `yaml` (one file), `team` (ZIP) or `folder`
    #[arg(long, default_value = "yaml", value_parser = FORMATS)]
    pub format: String,
    /// Where to write the package
    #[arg(long)]
    pub out: Option<String>,
    /// Also save the checked package to My templates
    #[arg(long)]
    pub save: bool,
    /// Review only: print the exact team.yaml and Includes list, write nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ExportArgs {
    /// Template name (`name[@version]`)
    pub name: String,
    /// Package form: `yaml` (one file), `team` (ZIP) or `folder`
    #[arg(long, default_value = "yaml", value_parser = FORMATS)]
    pub format: String,
    /// Where to write the package
    #[arg(long)]
    pub out: Option<String>,
}

#[derive(Args)]
pub struct ImportArgs {
    /// A `name.team.yaml` file, a `name.team` archive or a package folder
    pub path: String,
}

#[derive(Args)]
pub struct RecallArgs {
    /// What the Team should do, in plain words
    pub query: String,
}

#[derive(Args)]
pub struct DeleteArgs {
    /// Saved template name (`name[@version]`); without a version every version goes
    pub name: String,
}
