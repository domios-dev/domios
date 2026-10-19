use clap::Subcommand;

#[derive(Subcommand)]
pub enum TodoAction {
    /// Read this agent tab's current bounded checklist.
    List,
    /// Reconcile checklist statuses while retaining locked item ids.
    Write {
        /// JSON array of {content,status,activeForm,link?} from this tab's pad.
        #[arg(long, value_name = "PATH")]
        from_pad: String,
    },
}
