use clap::Subcommand;

#[derive(Subcommand)]
pub enum SurfaceAction {
    /// Open a plugin surface as a tab nested under its owner
    ///
    /// The owner defaults to the calling agent tab. The tab joins the owner's
    /// workspace and Team, so closing the Team closes the surface.
    Open {
        /// Surface to open, as `<plugin_id>:<surface_id>`
        target: String,
        /// Tab that owns the surface (default: the calling agent tab)
        #[arg(long, value_name = "TAB")]
        owner: Option<String>,
        /// Label the tab after its owner ("{owner}'s {title}") even when the surface is not agent-bound
        #[arg(long)]
        title_from_owner: bool,
    },
    /// List the surfaces enabled plugins declare and the surface tabs that are open
    List,
    /// Close a surface tab; refuses tabs that are not surfaces
    Close {
        /// Surface tab id
        #[arg(long, value_name = "TAB")]
        tab: String,
    },
}
