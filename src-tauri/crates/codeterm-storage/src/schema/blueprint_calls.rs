pub(super) const SCHEMA: &str = r#"
ALTER TABLE blueprint_step_journal ADD COLUMN call_stack_json TEXT
    CHECK (call_stack_json IS NULL OR (json_valid(call_stack_json) AND json_type(call_stack_json)='array' AND json_array_length(call_stack_json)<=8 AND length(call_stack_json)<=65536));
ALTER TABLE blueprint_step_journal ADD COLUMN claim_generation INTEGER;
ALTER TABLE blueprint_step_journal ADD COLUMN claim_parent_index INTEGER;
"#;

#[cfg(test)]
mod tests;

pub(super) const RUN_IDENTITY_SCHEMA: &str = "ALTER TABLE blueprint_step_journal ADD COLUMN definition_version INTEGER CHECK (definition_version IS NULL OR definition_version >= 0);";
