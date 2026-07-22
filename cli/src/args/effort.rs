use clap::Subcommand;

#[derive(Subcommand)]
pub enum EffortAction {
    /// Show the full effort bundle (linked spec(s) + plan + tasks) for one plan_id
    Show { plan_id: String },
    /// Link two efforts by lineage
    Link {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(
            long,
            value_name = "follows|supersedes",
            help = "Lineage relationship from --from to --to: follows (continues after) or supersedes (replaces)"
        )]
        rel: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::CommandFactory;

    #[test]
    fn effort_link_help_lists_rel_values() {
        let help = Cli::command()
            .find_subcommand_mut("effort")
            .unwrap()
            .find_subcommand_mut("link")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("follows"), "{help}");
        assert!(help.contains("supersedes"), "{help}");
    }
}
