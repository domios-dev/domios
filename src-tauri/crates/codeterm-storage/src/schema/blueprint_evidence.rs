pub(super) const SCHEMA: &str = r#"
CREATE TEMP TABLE blueprint_journal_hold AS SELECT * FROM blueprint_step_journal;
DROP TABLE blueprint_step_journal;
CREATE TABLE blueprint_step_journal (
    entry_id INTEGER PRIMARY KEY AUTOINCREMENT,
    blueprint_id TEXT NOT NULL REFERENCES blueprints(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL CHECK (length(run_id) BETWEEN 1 AND 128),
    step_index INTEGER NOT NULL CHECK (step_index >= -1),
    step_name TEXT NOT NULL CHECK (length(step_name) BETWEEN 1 AND 128),
    state TEXT NOT NULL CHECK (state IN ('running', 'done', 'failed', 'skipped', 'held')),
    outcome TEXT CHECK (outcome IS NULL OR length(outcome) BETWEEN 1 AND 128),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    ended_at_ms INTEGER CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms),
    result_summary TEXT CHECK (result_summary IS NULL OR length(result_summary) <= 200),
    reason TEXT CHECK (reason IS NULL OR length(reason) <= 200),
    body_ref TEXT CHECK (body_ref IS NULL OR length(body_ref) <= 32768),
    call_stack_json TEXT CHECK (call_stack_json IS NULL OR (json_valid(call_stack_json) AND json_type(call_stack_json)='array' AND json_array_length(call_stack_json)<=8 AND length(call_stack_json)<=65536)),
    claim_generation INTEGER,
    claim_parent_index INTEGER,
    definition_version INTEGER CHECK (definition_version IS NULL OR definition_version >= 0),
    evidence_json TEXT CHECK (evidence_json IS NULL OR (json_valid(evidence_json) AND json_type(evidence_json)='object' AND length(CAST(evidence_json AS BLOB))<=32768)),
    CHECK ((state = 'running') = (ended_at_ms IS NULL)),
    CHECK (step_index != -1 OR step_name IN ('$run', '$result'))
) STRICT;
INSERT INTO blueprint_step_journal (entry_id,blueprint_id,run_id,step_index,step_name,state,outcome,started_at_ms,ended_at_ms,result_summary,reason,body_ref,call_stack_json,claim_generation,claim_parent_index,definition_version) SELECT entry_id,blueprint_id,run_id,step_index,step_name,state,outcome,started_at_ms,ended_at_ms,result_summary,reason,body_ref,call_stack_json,claim_generation,claim_parent_index,definition_version FROM blueprint_journal_hold;
DROP TABLE blueprint_journal_hold;
CREATE INDEX blueprint_step_journal_newest ON blueprint_step_journal(blueprint_id, entry_id DESC);
"#;

#[cfg(test)]
mod tests;
