pub(super) const DOCUMENT_SCHEMA: &str =
    "ALTER TABLE blueprint_definitions ADD COLUMN document_json TEXT;";
pub(super) const CLEANUP: &str = r#"
CREATE TEMP TABLE obsolete_blueprint_versions AS
    SELECT blueprint_id, version FROM blueprint_definitions
    WHERE CASE WHEN json_valid(document_json) THEN json_extract(document_json, '$.schema_version') IS NOT 2 ELSE 1 END;
CREATE TABLE blueprint_v1_removal_receipts AS
    SELECT d.*, b.workspace_id, b.team_id, datetime('now') AS removed_at
    FROM blueprint_definitions d JOIN blueprints b ON b.id=d.blueprint_id
    WHERE (d.blueprint_id,d.version) IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions);
CREATE TEMP TABLE obsolete_blueprints AS
    SELECT id FROM blueprints WHERE NOT EXISTS (
        SELECT 1 FROM blueprint_definitions d WHERE d.blueprint_id=blueprints.id
        AND NOT EXISTS (SELECT 1 FROM obsolete_blueprint_versions o WHERE o.blueprint_id=d.blueprint_id AND o.version=d.version)
    );
CREATE TEMP TABLE obsolete_blueprint_transitions AS
    SELECT blueprint_id, transition_id FROM blueprint_transitions
    WHERE (blueprint_id, definition_version) IN (SELECT blueprint_id, version FROM obsolete_blueprint_versions);
UPDATE blueprints SET scenario_id = NULL WHERE scenario_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_command_cleanup WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_command_results WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_handler_results WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_transitions WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_handler_targets WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_handlers WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_buckets WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_heads WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_step_journal WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_definitions WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprint_runtime WHERE blueprint_id IN (SELECT id FROM obsolete_blueprints);
DELETE FROM blueprints WHERE id IN (SELECT id FROM obsolete_blueprints);
UPDATE blueprint_heads SET version=(
    SELECT max(d.version) FROM blueprint_definitions d WHERE d.blueprint_id=blueprint_heads.blueprint_id
    AND (d.blueprint_id,d.version) NOT IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions)
) WHERE (blueprint_id,version) IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions);
UPDATE blueprint_runtime SET held=NULL WHERE (blueprint_id,held) IN (SELECT blueprint_id,transition_id FROM obsolete_blueprint_transitions);
DELETE FROM blueprint_command_cleanup WHERE (blueprint_id,transition_id) IN (SELECT blueprint_id,transition_id FROM obsolete_blueprint_transitions);
DELETE FROM blueprint_command_results WHERE (blueprint_id,transition_id) IN (SELECT blueprint_id,transition_id FROM obsolete_blueprint_transitions);
DELETE FROM blueprint_handler_results WHERE (blueprint_id,transition_id) IN (SELECT blueprint_id,transition_id FROM obsolete_blueprint_transitions);
DELETE FROM blueprint_transitions WHERE (blueprint_id,transition_id) IN (SELECT blueprint_id,transition_id FROM obsolete_blueprint_transitions);
DELETE FROM blueprint_handler_targets WHERE (blueprint_id,version) IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions);
DELETE FROM blueprint_handlers WHERE (blueprint_id,version) IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions);
DELETE FROM blueprint_buckets WHERE (blueprint_id,version) IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions);
DELETE FROM blueprint_definitions WHERE (blueprint_id,version) IN (SELECT blueprint_id,version FROM obsolete_blueprint_versions);
DROP TABLE obsolete_blueprint_transitions;
DROP TABLE obsolete_blueprint_versions;
DROP TABLE obsolete_blueprints;
"#;
#[cfg(test)]
mod tests;
