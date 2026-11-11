use clap::Subcommand;

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
    Start {
        /// Template name (`name[@version]`)
        name: String,
        #[arg(long, env = "CODETERM_WORKSPACE_ID")]
        workspace: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
    },
}
