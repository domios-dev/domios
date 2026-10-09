# Blueprint release boundary inventory

Observed source inventory for the 1.14.0 rename. Internal Rust modules, functions,
types, and existing opaque IDs retain their names. New IDs use `blueprint_`.

| Boundary | Legacy name | Canonical name / compatibility |
| --- | --- | --- |
| SQLite identity table | `sentinels` | `blueprints` |
| SQLite definition tables | `sentinel_definitions`, `sentinel_heads`, `sentinel_buckets`, `sentinel_handlers`, `sentinel_handler_targets` | Corresponding `blueprint_*` tables |
| SQLite runtime/history tables | `sentinel_runtime`, `sentinel_transitions`, `sentinel_handler_results`, `sentinel_command_results`, `sentinel_command_cleanup` | Corresponding `blueprint_*` tables |
| SQLite foreign identity column | `sentinel_id` in all ten child tables and `blueprint_step_journal` | `blueprint_id` |
| SQLite identity indexes | `sentinels_by_scope`, `sentinels_by_team`, `sentinels_by_scenario` | Corresponding `blueprints_*` indexes |
| Stored handler / historical target kind | `sentinel` | `blueprint`; migration preserves references |
| Portable `.blueprint.json` | Handler `type: sentinel`, route `sentinel:<id>`, action/identity `kind: sentinel` | Writes Blueprint vocabulary; importer accepts both |
| Portable envelope | `schema_version`, `placeholders`, `definition`, `create`, `spec` | Already canonical; no rename needed |
| Graph JSON | Identity/action `kind: sentinel`, `sentinel_definition` | `blueprint`, `blueprint_definition` |
| History/escalation/activity JSON | `sentinel_id`, activity kind `sentinel` | `blueprint_id`, `blueprint`; history and feed accept old keys/kinds |
| REST | Existing `/api/automation/blueprints/{id}` DELETE and `/{id}/stop` POST | Canonical paths retained; `/api/automation/sentinels/...` and `/api/sentinels/...` are aliases |
| REST graph/import/export | `/api/automation/graph`, `/api/automation/graph/workspace`, `/api/automation/blueprints/import`, `/export` | Already canonical |
| Socket RPC | `sentinel.<operation>` | Canonical `blueprint.<operation>`; prefix alias dispatches through the same authorization path |
| Tauri IPC | `automation_graph_stop_sentinel`, `automation_graph_delete_sentinel` | Blueprint wrappers canonical; old commands retained as aliases |
| External errors/reasons | `sentinel_store_*`, `sentinel_version_conflict`, `sentinel_prompt_*`, `sentinel_observation_*` | Corresponding `blueprint_*` codes |
| Routed message identity | `sentinel:<id>` reminder origin | `blueprint:<id>` |
| CLI commands/flags | Blueprint CLI already canonical | History output reads `blueprint_id`, with old-key fallback |
| Config keys | No Sentinel keys found in `codeterm-types/src/config` | No change required |
| Desk/snapshot fields | No Sentinel fields found in desk schema or persisted multiplexer snapshots | No change required |
| Legacy import manifest domain | `sentinel` | Writes `blueprint`; manifest parser accepts both |
| Legacy file import | `sentinels.json`, historical backup/capture paths | Read-only source compatibility; new runtime persistence uses SQLite |

One appended schema migration renames the tables and columns, rebuilds the handler
CHECK constraint to admit the canonical action kind, and checks foreign keys before
commit. Historical migrations remain byte-compatible with pre-release fixtures.

Focused verification selectors cover the
pre-rename SQLite fixture, persisted deletion through the REST alias, old-key
portable import, and the existing Blueprint/Sentinel suites.

The same migration finalizes source_kind for probe, observe, input, terminal and
pipeline, adds the journal body_ref column, and admits typed held run closes.
The journal retains 32 complete runs; log limits select runs, preserving all rows
of a selected run. Result references formerly packed into summaries migrate into
their dedicated column.
