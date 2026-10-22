use super::*;

pub(crate) fn update_definition(
    cx: &mut WriteCx<'_>,
    id: &SentinelId,
    expect_version: u64,
    update: DefinitionUpdate,
) -> Result<SentinelCommit, StorageError> {
    let id = id.as_str();
    let Some(head) = definitions::head_version(cx.conn, id)? else {
        return Ok(SentinelCommit::fenced(Fence::Missing, None));
    };
    if head != expect_version {
        return fenced(cx.conn, Fence::Version, id);
    }
    let next = definitions::latest_version(cx.conn, id)? + 1;
    let input_due_at = update
        .definition
        .pipeline()
        .is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
            )
        })
        .then_some(update.authored_at);
    let amendment = AmendmentRecord {
        version: next,
        replaced_version: head,
        author_tab_id: update.author_tab_id,
        cause_transition_id: update.cause_transition_id,
        authored_at: update.authored_at,
    };
    definitions::insert_version(
        cx,
        id,
        next,
        DefinitionProvenance::Authored,
        Some(&amendment),
        &update.definition,
    )?;
    definitions::move_head(cx, id, next)?;
    cx.write(
        "UPDATE blueprint_runtime SET transition_offset = (SELECT COUNT(*) FROM blueprint_transitions WHERE blueprint_id = ?1),
         action_offset = (SELECT COUNT(*) FROM blueprint_handler_results WHERE blueprint_id = ?1 AND claim_id IS NOT NULL),
         stopped = 0, stopped_at = NULL, paused = 0, last_hash = NULL, last_bucket = NULL, last_reason = NULL,
         held = NULL, run_transitions = 0, runs_since_transition = 0, last_probe_at = NULL, probe_state = NULL,
         expires_at = NULL, next_run = ?2 WHERE blueprint_id = ?1 AND stopped = 1",
        params![id, to_sql_u64("restart time", update.authored_at)?],
    )?;
    let advanced = if let Some(next_run) = input_due_at {
        cx.write(
            "UPDATE blueprint_runtime SET generation = generation + 1, next_run = ?2 WHERE blueprint_id = ?1",
            params![id, to_sql_u64("input amendment due time", next_run)?],
        )?
    } else {
        cx.write(
            "UPDATE blueprint_runtime SET generation = generation + 1 WHERE blueprint_id = ?1",
            [id],
        )?
    };
    if advanced != 1 {
        return Err(corrupt("stage has no runtime"));
    }
    touch(cx, id)?;
    finished(cx.conn, MutationOutcome::Applied, &[id.to_string()])
}
