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
        /// Also remove plugin-owned downloaded data.
        #[arg(long = "remove-data")]
        remove_data: bool,
    },
    /// Manage plugin channels.
    Channel {
        #[command(subcommand)]
        command: PluginChannelAction,
    },
    /// Show a plugin's agent-facing config: its self-documenting config.yaml
    /// (secret values redacted), settings schema, and installed/configured/
    /// reload state. The agent's discover handle (§1.7).
    Get { id: String },
    /// Configure a plugin by editing its self-documenting config.yaml (comments
    /// preserved, validated against the settings schema). Non-secret fields are
    /// written to the YAML; secret fields are reported for routing to the secret
    /// store (never written plaintext). Reload is live.
    Config {
        id: String,
        /// Set a field: --set key=value (repeatable).
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set: Vec<String>,
        /// Set multiple fields from a JSON object (named --values-json so it does
        /// not collide with the global --json output flag).
        #[arg(long = "values-json", value_name = "JSON")]
        values_json: Option<String>,
        /// Grant a permission, persisted to the plugin's grants.yaml so the
        /// decision survives (e.g. --grant spawn_agents). Repeatable. Takes
        /// effect on the plugin's next reload.
        #[arg(long = "grant", value_name = "PERMISSION")]
        grant: Vec<String>,
        /// Revoke a previously granted permission (repeatable).
        #[arg(long = "revoke", value_name = "PERMISSION")]
        revoke: Vec<String>,
    },
    /// Validate a plugin directory without installing it: strict manifest parse
    /// (the daemon's deny-unknown-fields PluginManifest), a real QuickJS load of
    /// the entry script, then a report of which AgentProvider exports plugin.js
    /// implements and which fallbacks serve the rest. Exits non-zero on the
    /// first failure, naming it.
    Validate {
        /// Path to the plugin directory (the folder containing plugin.json).
        dir: String,
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
