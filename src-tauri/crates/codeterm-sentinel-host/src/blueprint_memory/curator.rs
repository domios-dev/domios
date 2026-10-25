use super::*;
use codeterm_blueprint_types::{blueprint_document::Document, blueprint_memory as model};
use codeterm_service_core::service::AuthenticatedPlanCaller;
use std::collections::BTreeSet;

pub(crate) fn candidates(
    handle: &Handle,
) -> std::collections::BTreeMap<String, model::curator::Candidate> {
    let jobs = handle.store.lock().list().to_vec();
    let mut groups = std::collections::BTreeMap::<String, Vec<String>>::new();
    for job in jobs.iter().filter(|job| job.machine.stopped) {
        if !evidence(handle, job).is_ok_and(|proof| proof.successes > 0) {
            continue;
        }
        let Ok(document) = Document::from_definition(&crate::portable::definition(job)) else {
            continue;
        };
        let key = format!(
            "{}:{:?}:{}",
            job.workspace_id,
            job.team_id,
            model::structure(&model::abstract_document(document))
        );
        groups
            .entry(model::curator::fingerprint(&key))
            .or_default()
            .push(job.id.clone());
    }
    groups
        .into_iter()
        .flat_map(|(shape, ids)| {
            let episodes = ids.iter().cloned().collect::<BTreeSet<_>>();
            ids.into_iter().map(move |id| {
                (
                    id,
                    model::curator::Candidate {
                        shape: shape.clone(),
                        episodes: episodes.clone(),
                    },
                )
            })
        })
        .collect()
}

pub(super) fn keep(
    ctx: &ServiceContext,
    caller: &SentinelCaller,
    id: &str,
    when: &[String],
    tags: &[String],
) -> Result<Value, String> {
    let session = {
        let mux = ctx.mux.read();
        if !caller_is_live(&mux, caller) {
            return Err("Curator caller binding changed".into());
        }
        mux.get_tab(&caller.tab)
            .and_then(|tab| tab.agent_session_id.clone())
    };
    let identity = ctx.curator_identity(&AuthenticatedPlanCaller {
        tab_id: Some(caller.tab.clone()),
        session_id: session,
        binding_nonce: caller.nonce.clone(),
    })?;
    if ctx
        .plan_store
        .get(&identity.plan_id)
        .is_none_or(|plan| plan.team_id != caller.team)
    {
        return Err("Curator Blueprint belongs to another Team".into());
    }
    let handle = ctx
        .host_extension::<Handle>()
        .ok_or("blueprints not initialized")?;
    let jobs = {
        let store = handle.store.lock();
        store.ensure_available()?;
        store
            .list()
            .iter()
            .filter(|job| job.workspace_id == caller.workspace && job.team_id == caller.team)
            .cloned()
            .collect::<Vec<_>>()
    };
    let job = jobs
        .iter()
        .find(|job| job.id == id)
        .ok_or("Blueprint not found in this Team")?;
    let document = Document::from_definition(&crate::portable::definition(job))?;
    let shape = model::structure(&model::abstract_document(document.clone()));
    let mut episodes = BTreeSet::new();
    for episode in &jobs {
        if !episode.machine.stopped || evidence(&handle, episode)?.successes == 0 {
            continue;
        }
        let other = Document::from_definition(&crate::portable::definition(episode))?;
        if model::structure(&model::abstract_document(other)) == shape {
            episodes.insert(episode.id.clone());
        }
    }
    let mut proof = evidence(&handle, job)?;
    proof.tags = tags.to_vec();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let shape_key =
        model::curator::fingerprint(&format!("{}:{:?}:{}", job.workspace_id, job.team_id, shape));
    let mut evicted_ids = Vec::new();
    let saved = ctx.curator_save_solution(
        &identity,
        id,
        &shape_key,
        episodes,
        now,
        |episodes, granted| {
            proof.curator = Some(model::CuratorEvidence {
                plan: identity.plan_id.clone(),
                round: identity.number,
                episodes: episodes.iter().cloned().collect(),
                granted,
            });
            let saved = ctx.mem.save_blueprint_scoped(
                &area(&caller.workspace),
                caller
                    .team
                    .as_deref()
                    .ok_or("Saved callable requires a Team")?,
                document,
                when,
                proof,
            )?;
            evicted_ids = saved.evicted_ids;
            Ok(saved.id)
        },
    )?;
    Ok(json!({"id":saved,"evicted_ids":evicted_ids,"status":"saved","tags":tags}))
}
