use super::*;
use crate::service::remote_tabs::{merge_ordinary_tab_projection, RemotePeerProjection};

fn member() -> RemoteTeamMember {
    RemoteTeamMember {
        peer_id: "peer".into(),
        tab_id: "worker".into(),
        session_id: "session".into(),
        title: "Worker".into(),
        provider: "provider".into(),
        orchestrator_tab_id: "leader".into(),
        team_id: "team".into(),
        workspace_id: "workspace".into(),
        operation_id: "operation".into(),
        finalized: false,
    }
}

fn project(sources: Vec<Value>, member: &RemoteTeamMember) -> Vec<Value> {
    let peer = RemotePeerProjection {
        peer_id: "peer".into(),
        peer_name: "Peer".into(),
        authenticated: true,
        reachable: false,
        availability: "reconnecting",
        tabs: member_sources(sources, std::slice::from_ref(member)),
        controllers: std::collections::HashMap::from([("worker".into(), "leader".into())]),
    };
    merge_ordinary_tab_projection(&[], &[peer])
}

#[test]
fn an_offline_member_is_a_real_team_row_without_a_report_or_local_controller() {
    let rows = project(Vec::new(), &member());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "mesh:peer/worker");
    assert_eq!(rows[0]["team_id"], "team");
    assert_eq!(rows[0]["workspace_id"], "workspace");
    assert_eq!(rows[0]["parent_tab_id"], "leader");
    assert_eq!(rows[0]["remote_availability"], "unavailable");
}

#[test]
fn a_replacement_session_never_inherits_the_members_control_surface() {
    let rows = project(
        vec![
            json!({"id":"worker","agent_type":"provider","agent_session_id":"replacement","pty_id":"live-replacement-pty"}),
        ],
        &member(),
    );
    assert_eq!(rows[0]["agent_session_id"], "session");
    assert_eq!(rows[0]["pty_id"], Value::Null);
    assert_eq!(rows[0]["remote_availability"], "unavailable");
}

#[test]
fn finalized_members_cannot_reappear_from_cached_peer_tabs() {
    let mut member = member();
    member.finalized = true;
    assert!(project(vec![json!({"id":"worker","agent_type":"provider","agent_session_id":"session","pty_id":"old-pty"})], &member).is_empty());
}

#[test]
fn the_matching_worker_keeps_its_session_and_uses_durable_team_placement() {
    let rows = project(
        vec![
            json!({"id":"worker","agent_type":"provider","agent_session_id":"session","pty_id":"remote-pty","team_id":"foreign-team"}),
        ],
        &member(),
    );
    assert_eq!(rows[0]["agent_session_id"], "session");
    assert_eq!(rows[0]["team_id"], "team");
    assert_eq!(rows[0]["pty_id"], "mesh:peer/worker");
}
