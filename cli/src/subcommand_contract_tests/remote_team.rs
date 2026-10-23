use super::*;
use crate::args::team::TeamAction;

#[test]
fn adoption_names_the_remote_worker_peer_without_moving_the_team() {
    let cli = Cli::try_parse_from([
        "domios",
        "team",
        "adopt-agent",
        "local-team",
        "worker",
        "--peer",
        "remote",
    ])
    .unwrap();
    assert!(
        matches!(cli.command, Commands::Team { action: TeamAction::AdoptAgent { team_id, tab_id, peer: Some(peer) } } if team_id == "local-team" && tab_id == "worker" && peer == "remote")
    );
}

#[test]
fn reparent_requires_an_explicit_local_destination_and_accepts_a_worker_peer() {
    assert!(Cli::try_parse_from(["domios", "agent", "reparent", "worker"]).is_err());
    for peer_args in [vec![], vec!["--peer", "remote"]] {
        let mut args = vec![
            "domios",
            "agent",
            "reparent",
            "worker",
            "--to",
            "orchestrator",
        ];
        args.extend(peer_args);
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(cli.command, Commands::Agent { action: AgentAction::Reparent { tab, to, .. } } if tab == "worker" && to == "orchestrator")
        );
    }
}
