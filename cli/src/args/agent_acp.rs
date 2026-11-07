use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum AcpAction {
    /// List configured ACP profiles and their installation and authentication state
    List,
    /// Add an owner-configured installed ACP command (local owner or Orchestrator only)
    Add {
        name: String,
        #[arg(long)]
        binary: String,
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        /// Non-secret environment entries as KEY=VALUE
        #[arg(long = "env")]
        env: Vec<String>,
        /// Required secrets as KEY=secret-name; never pass secret values
        #[arg(long = "secret-env")]
        secret_env: Vec<String>,
        /// Optional sign-in secrets as KEY=secret-name:method
        #[arg(long = "auth-env")]
        auth_env: Vec<String>,
        /// Copy the icon, client capabilities and limits source declared by this profile
        #[arg(long)]
        like: Option<String>,
        /// Override a declared client capability as NAME=true|false (e.g. fs.readTextFile=false)
        #[arg(long = "client-capability")]
        client_capabilities: Vec<String>,
        /// Where account limits come from: account:PROVIDER, balance:READER:ENV or unavailable:REASON
        #[arg(long = "limits-source")]
        limits_source: Option<String>,
    },
    /// Remove an owner-added ACP profile
    Remove { name: String },
    /// Discover and install pinned agents from the ACP registry
    Registry {
        #[command(subcommand)]
        action: AcpRegistryAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum AcpRegistryAction {
    /// Search the ACP registry
    Search { query: String },
    /// Install the registry's exact declared distribution as an owner profile
    Install {
        id: String,
        /// Override the plugin's declared default for this registry id: account:PROVIDER, balance:READER:ENV or unavailable:REASON
        #[arg(long = "limits-source")]
        limits_source: Option<String>,
    },
}
