//! Exhaustive compile-time transport audit for every mem action.

use crate::{MemAction, MemGraphAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemTransport {
    /// Routed through the generic socket dispatch table.
    Socket,
    /// Socket-routed through a dedicated CLI handler.
    SocketDedicated,
    /// Never touches the daemon at all — opens knowledge.db directly.
    LocalOnly,
    /// Short-circuited before socket connection.
    ShortCircuitedInMain,
}

pub(crate) fn classify(action: &MemAction) -> MemTransport {
    use MemTransport::*;
    match action {
        MemAction::Search { .. }
        | MemAction::Get { .. }
        | MemAction::Delete { .. }
        | MemAction::Forget { .. }
        | MemAction::Update { .. }
        | MemAction::Append { .. }
        | MemAction::Restore { .. }
        | MemAction::Pin { .. }
        | MemAction::Unpin { .. }
        | MemAction::Pinned { .. }
        | MemAction::Save { .. }
        | MemAction::SavePreference { .. }
        | MemAction::WakeUp { .. }
        | MemAction::ExtractPending { .. }
        | MemAction::Stats
        | MemAction::Learn { .. }
        | MemAction::LearnList
        | MemAction::LearnPromote { .. }
        | MemAction::Flush { .. }
        | MemAction::Sync { .. }
        | MemAction::Reembed { .. } => Socket,

        MemAction::History { .. }
        | MemAction::Taxonomy { .. }
        | MemAction::Tail { .. }
        | MemAction::Skill { .. }
        | MemAction::Worker { .. }
        | MemAction::Calibration { .. } => SocketDedicated,

        MemAction::Graph { action } => classify_graph(action),

        MemAction::RunWorker { .. } | MemAction::Dream { .. } => ShortCircuitedInMain,

        MemAction::Secret { .. } => SocketDedicated,
    }
}

fn classify_graph(action: &MemGraphAction) -> MemTransport {
    use MemTransport::*;
    match action {
        MemGraphAction::Route { .. }
        | MemGraphAction::PurgeInvalidated { .. }
        | MemGraphAction::ReviveBroken { .. }
        | MemGraphAction::IntroduceTopic { .. }
        | MemGraphAction::Hubs { .. }
        | MemGraphAction::Islands { .. }
        | MemGraphAction::Orphans { .. }
        | MemGraphAction::Get { .. } => LocalOnly,

        MemGraphAction::Create { .. }
        | MemGraphAction::Relate { .. }
        | MemGraphAction::Related { .. }
        | MemGraphAction::Invalidate { .. }
        | MemGraphAction::Timeline { .. }
        | MemGraphAction::Traverse { .. }
        | MemGraphAction::Path { .. }
        | MemGraphAction::Tunnels { .. }
        | MemGraphAction::Alias { .. }
        | MemGraphAction::FindDuplicateRelations { .. }
        | MemGraphAction::DeleteRelations { .. }
        | MemGraphAction::MergeRelations { .. }
        | MemGraphAction::FindDuplicateEntities { .. }
        | MemGraphAction::AugmentEntity { .. }
        | MemGraphAction::RepairOrphanVectors { .. }
        | MemGraphAction::PruneThinVectors { .. }
        | MemGraphAction::CollapseSynonymEdges { .. }
        | MemGraphAction::PruneDreamHubEdges { .. }
        | MemGraphAction::RepairOrphanBlobs { .. }
        | MemGraphAction::DeleteEntity { .. } => SocketDedicated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> MemAction {
        let cli = crate::Cli::try_parse_from(args).unwrap();
        match cli.command {
            crate::Commands::Mem { action } => action,
            _ => panic!("expected a mem command for {args:?}"),
        }
    }

    #[test]
    fn item_surface_classifies_as_socket() {
        for args in [
            vec!["codeterm", "mem", "get", "fact_a"],
            vec![
                "codeterm", "mem", "save", "--kind", "fact", "--title", "t", "b",
            ],
            vec!["codeterm", "mem", "wake-up"],
            vec!["codeterm", "mem", "learn-list"],
        ] {
            assert_eq!(classify(&parse(&args)), MemTransport::Socket, "{args:?}");
        }
    }

    #[test]
    fn pretty_printed_and_local_io_actions_classify_as_socket_dedicated() {
        for args in [
            vec!["codeterm", "mem", "history", "fact_a"],
            vec!["codeterm", "mem", "taxonomy"],
            vec!["codeterm", "mem", "tail"],
            vec!["codeterm", "mem", "skill", "list"],
            vec!["codeterm", "mem", "worker", "list"],
            vec!["codeterm", "mem", "calibration", "sweep"],
        ] {
            assert_eq!(
                classify(&parse(&args)),
                MemTransport::SocketDedicated,
                "{args:?}"
            );
        }
    }

    #[test]
    fn graph_repair_and_query_actions_classify_as_socket_dedicated() {
        for args in [
            vec![
                "codeterm",
                "mem",
                "graph",
                "create",
                "--id",
                "person_a",
                "--entity-type",
                "Person",
            ],
            vec!["codeterm", "mem", "graph", "related", "--id", "person_a"],
            vec!["codeterm", "mem", "graph", "repair-orphan-vectors"],
        ] {
            let MemAction::Graph { action } = parse(&args) else {
                panic!("expected graph action for {args:?}");
            };
            assert_eq!(
                classify_graph(&action),
                MemTransport::SocketDedicated,
                "{args:?}"
            );
        }
    }

    #[test]
    fn direct_db_graph_actions_classify_as_local_only() {
        for args in [
            vec!["codeterm", "mem", "graph", "hubs"],
            vec!["codeterm", "mem", "graph", "orphans"],
            vec!["codeterm", "mem", "graph", "get", "person_a"],
        ] {
            let MemAction::Graph { action } = parse(&args) else {
                panic!("expected graph action for {args:?}");
            };
            assert_eq!(classify_graph(&action), MemTransport::LocalOnly, "{args:?}");
        }
    }

    #[test]
    fn secret_uses_same_user_socket_transport() {
        let action = parse(&["codeterm", "mem", "secret", "status"]);
        assert_eq!(classify(&action), MemTransport::SocketDedicated);
    }

    #[test]
    fn short_circuited_actions_never_reach_socket_dispatch() {
        let run_worker = parse(&["codeterm", "mem", "run-worker", "librarian-once"]);
        assert_eq!(classify(&run_worker), MemTransport::ShortCircuitedInMain);

        let dream = parse(&["codeterm", "mem", "dream", "run"]);
        assert_eq!(classify(&dream), MemTransport::ShortCircuitedInMain);
    }
}
