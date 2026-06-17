use clap::Subcommand;

#[derive(Clone, Subcommand)]
pub enum PluginAction {
    /// List installed plugins.
    List,
    /// Install a plugin by id from channels, or directly from a local folder.
    Install {
        /// Plugin id to install from registered channels.
        id: Option<String>,
        /// Install from a local plugin directory.
        #[arg(long = "from-dir")]
        from_dir: Option<String>,
        /// Override the install id. Defaults to plugin.json#id or folder name.
        #[arg(long = "id-override")]
        id_override: Option<String>,
        /// Disambiguate channel installs when more than one channel offers the id.
        #[arg(long)]
        channel: Option<String>,
    },
    /// Update one installed plugin, or all plugins when id is omitted.
    Update { id: Option<String> },
    /// Update bundled system provider plugins from the official plugin channel.
    #[command(alias = "system-update")]
    UpdateSystem { id: Option<String> },
    /// Remove an installed plugin.
    Remove {
        id: String,
        /// Allow removing a verified/system plugin folder.
        #[arg(long)]
        force: bool,
    },
    /// Manage plugin channels.
    Channel {
        #[command(subcommand)]
        command: PluginChannelAction,
    },
    /// Passthrough to a plugin's `onAgentCommand`: `codeterm plugin <id> <verb>
    /// [args…]`. Routes the reserved `plugin:<id>.<verb>` socket method to the
    /// owning plugin (requires its `agent_commands` grant). Catch-all for any
    /// first token that isn't one of the built-in subcommands above.
    #[command(external_subcommand)]
    Pass(Vec<String>),
}

#[derive(Clone, Subcommand)]
pub enum PluginChannelAction {
    /// Add a local path or git URL channel.
    Add {
        url: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Register the default marketplace channel (codeterm-plugins) if missing.
    /// Register-only: nothing is installed. Best-effort/offline-tolerant.
    EnsureDefault,
    /// List registered channels.
    List,
    /// Refresh one channel, or all channels when name is omitted.
    Refresh { name: Option<String> },
    /// Compare installed plugins against a channel without installing.
    Diff { name: Option<String> },
    /// Remove a channel cache and registry entry.
    Remove {
        name: String,
        /// Allow removing the protected system channel registration.
        #[arg(long)]
        force: bool,
    },
}
