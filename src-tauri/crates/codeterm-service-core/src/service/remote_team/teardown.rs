use super::{
    member_key, RemoteFinalizationReceipt, RemoteTeamFinalization, RemoteTeamMember,
    RemoteTeamPeer, RemoteWorkerFinalize,
};
use crate::service::ServiceContext;

impl ServiceContext {
    pub fn prepare_remote_team_close(
        &self,
        team: &str,
    ) -> Result<(Vec<RemoteTeamMember>, bool), String> {
        let local_members = self.mux.read().team_member_tab_ids(team);
        self.remote_teams.update(|data| {
            if data.outgoing.values().any(|operation| {
                !operation.completed
                    && (operation.member.team_id == team
                        || operation
                            .actor_tab_id
                            .as_ref()
                            .is_some_and(|id| local_members.contains(id))
                        || data
                            .members
                            .get(&member_key(
                                &operation.peer_id,
                                &operation.request.worker_tab_id,
                            ))
                            .is_some_and(|member| member.team_id == team))
            }) {
                return Err(
                    "Team has an unresolved remote Worker transfer; recover it before closing"
                        .into(),
                );
            }
            let started = data.closing_teams.insert(team.into());
            Ok((
                data.members
                    .values()
                    .filter(|member| member.team_id == team)
                    .cloned()
                    .collect(),
                started,
            ))
        })
    }

    pub async fn finalize_remote_team_members(
        &self,
        team: &str,
        force: bool,
    ) -> Result<Vec<RemoteTeamFinalization>, String> {
        let service = self.clone();
        let team_id = team.to_string();
        let (members, newly_started) =
            crate::service::blocking::run(move || service.prepare_remote_team_close(&team_id))
                .await?;
        let preparation = async {
            let mut peers = Vec::new();
            for member in members.iter().filter(|member| !member.finalized) {
                let (_, peer) = self
                    .remote_team_transport(&member.peer_id)
                    .map_err(|error| format!("{}@{}: {error}", member.tab_id, member.peer_id))?;
                peers.push((member.clone(), peer));
            }
            let mut ready = Vec::new();
            for (member, peer) in peers {
                let receipt = probe_member(&member, force, true, &peer).await?;
                ready.push((member, peer, receipt));
            }
            Ok::<_, String>(ready)
        }
        .await;
        let ready = match preparation {
            Ok(ready) => ready,
            Err(error) => {
                if newly_started {
                    let store = self.remote_teams.clone();
                    let team = team.to_string();
                    crate::service::blocking::run(move || {
                        store.update(|data| {
                            data.closing_teams.remove(&team);
                            Ok(())
                        })
                    })
                    .await?;
                }
                return Err(format!(
                    "remote Team preflight failed; Team and membership retained: {error}"
                ));
            }
        };
        for (member, peer, preflight) in ready {
            let receipt = if preflight.outcome == "ready" {
                probe_member(&member, force, false, &peer).await?
            } else {
                preflight
            };
            self.persist_remote_finalization(&member, &receipt).await?;
        }
        let store = self.remote_teams.clone();
        let team = team.to_string();
        crate::service::blocking::run(move || {
            let data = store.snapshot()?;
            data.members
                .values()
                .filter(|member| member.team_id == team)
                .map(|member| {
                    data.finalizations
                        .get(&member_key(&member.peer_id, &member.tab_id))
                        .cloned()
                        .ok_or_else(|| "remote member finalization is incomplete".into())
                })
                .collect()
        })
        .await
    }

    pub async fn finalize_remote_team_member_with_peer(
        &self,
        member: &RemoteTeamMember,
        force: bool,
        peer: &impl RemoteTeamPeer,
    ) -> Result<RemoteTeamFinalization, String> {
        let data = self.remote_teams.snapshot()?;
        if !data.closing_teams.contains(&member.team_id)
            || data
                .members
                .get(&member_key(&member.peer_id, &member.tab_id))
                != Some(member)
        {
            return Err("remote Team member has no exact close reservation".into());
        }
        let receipt = probe_member(member, force, false, peer).await?;
        self.persist_remote_finalization(member, &receipt).await
    }

    async fn persist_remote_finalization(
        &self,
        member: &RemoteTeamMember,
        receipt: &RemoteFinalizationReceipt,
    ) -> Result<RemoteTeamFinalization, String> {
        let member = member.clone();
        let orchestrator = member.orchestrator_tab_id.clone();
        let result = RemoteTeamFinalization {
            peer_id: member.peer_id.clone(),
            tab_id: receipt.worker_tab_id.clone(),
            session_id: receipt.worker_session_id.clone(),
            provider: member.provider.clone(),
            outcome: receipt.outcome.clone(),
        };
        let saved = result.clone();
        let store = self.remote_teams.clone();
        crate::service::blocking::run(move || {
            store.update(|data| {
                if !data.closing_teams.contains(&member.team_id) {
                    return Err("remote Team has no close reservation".into());
                }
                let key = member_key(&member.peer_id, &member.tab_id);
                let current = data
                    .members
                    .get_mut(&key)
                    .ok_or("remote Team member disappeared")?;
                if current.operation_id != member.operation_id
                    || current.session_id != member.session_id
                {
                    return Err("remote Team member identity changed during close".into());
                }
                current.finalized = true;
                data.finalizations.insert(key, saved);
                Ok(())
            })
        })
        .await?;
        self.emit_remote_workers_changed(&orchestrator);
        self.forget_remote_member_telemetry(&result.peer_id, &result.tab_id)
            .await?;
        Ok(result)
    }

    pub fn complete_remote_team_close(&self, team: &str) -> Result<(), String> {
        self.remote_teams.update(|data| {
            if data
                .members
                .values()
                .any(|member| member.team_id == team && !member.finalized)
            {
                return Err("remote Team still has unfinalized members".into());
            }
            let keys = data
                .members
                .iter()
                .filter(|(_, member)| member.team_id == team)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in keys {
                data.members.remove(&key);
            }
            data.closing_teams.remove(team);
            Ok(())
        })
    }

    pub fn restore_remote_team_closures(&self) {
        let Ok(data) = self.remote_teams.snapshot() else {
            return;
        };
        let gone = {
            let mux = self.mux.read();
            data.closing_teams
                .iter()
                .filter(|team| mux.get_team(team).is_none())
                .cloned()
                .collect::<Vec<_>>()
        };
        for team in gone {
            if let Err(error) = self.complete_remote_team_close(&team) {
                log::error!("remote Team close recovery retained membership: {error}");
            }
        }
    }
}

async fn probe_member(
    member: &RemoteTeamMember,
    force: bool,
    check_only: bool,
    peer: &impl RemoteTeamPeer,
) -> Result<RemoteFinalizationReceipt, String> {
    let request = RemoteWorkerFinalize {
        worker_tab_id: member.tab_id.clone(),
        worker_session_id: member.session_id.clone(),
        worker_provider: member.provider.clone(),
        expected_parent_tab_id: member.orchestrator_tab_id.clone(),
        force,
        check_only,
    };
    let receipt: RemoteFinalizationReceipt = serde_json::from_value(
        peer.post("/api/mesh/worker-finalize", serde_json::json!(request))
            .await
            .map_err(|error| format!("{}@{}: {error}", member.tab_id, member.peer_id))?,
    )
    .map_err(|error| error.to_string())?;
    let allowed = matches!(
        receipt.outcome.as_str(),
        "closed" | "absent" | "released_stale_binding"
    ) || (check_only && receipt.outcome == "ready");
    if receipt.worker_tab_id != member.tab_id
        || receipt.worker_session_id != member.session_id
        || !allowed
    {
        return Err("remote Team finalization receipt does not match its member".into());
    }
    Ok(receipt)
}
