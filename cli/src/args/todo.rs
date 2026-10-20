use clap::Subcommand;

#[derive(Subcommand)]
pub enum TodoAction {
    /// Read this agent tab's current bounded checklist.
    List,
    /// Declare this tab's whole checklist: created on first write, then reconciled; locked items stay.
    Write {
        /// JSON array of {content,status,activeForm,link?,reason?} from this tab's pad.
        #[arg(long, value_name = "PATH")]
        from_pad: String,
    },
}
