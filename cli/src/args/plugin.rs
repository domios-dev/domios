#[derive(Clone)]
pub enum PluginAction {
    /// List installed plugins.
    List,
    /// Install a plugin by id from channels, or directly from a local folder.
    Install {
        /// Plugin id to install from registered channels.
        id: Option<String>,
        /// Install from a local plugin directory.
        from_dir: Option<String>,
        /// Override the install id. Defaults to plugin.json#id or folder name.
        id_override: Option<String>,
        /// Disambiguate channel installs when more than one channel offers the id.
        channel: Option<String>,
    },
    /// Update one installed plugin, or all plugins when id is omitted.
    Update {
        id: Option<String>,
    },
    /// Update bundled system provider plugins from the official plugin channel.
    UpdateSystem {
        id: Option<String>,
    },
    /// Remove an installed plugin.
    Remove {
        id: String,
        /// Allow removing a verified/system plugin folder.
        force: bool,
    },
    /// Manage plugin channels.
    Channel {
        command: PluginChannelAction,
    },
}

#[derive(Clone)]
pub enum PluginChannelAction {
    /// Add a local path or git URL channel.
    Add {
        url: String,
        name: Option<String>,
    },
    /// List registered channels.
    List,
    /// Refresh one channel, or all channels when name is omitted.
    Refresh {
        name: Option<String>,
    },
    /// Compare installed plugins against a channel without installing.
    Diff {
        name: Option<String>,
    },
    /// Remove a channel cache and registry entry.
    Remove {
        name: String,
        /// Allow removing the protected system channel registration.
        force: bool,
    },
}
