use clap::Args;

const FORMATS: [&str; 3] = ["yaml", "team", "folder"];

#[derive(Args)]
pub struct DistillArgs {
    /// Team ID
    pub team_id: String,
    /// Package form: `yaml` (one file), `team` (ZIP) or `folder`
    #[arg(long, default_value = "yaml", value_parser = FORMATS)]
    pub format: String,
    /// Where to write the package
    #[arg(long)]
    pub out: Option<String>,
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
