# v1.1.23 E2E damage manifest

- Injected at: 2026-05-08T12:15:58.182008Z
- Target DB: C:\Users\dev\.codeterm-dev\mem\knowledge.db
- forced valid_to: `2025-01-01T00:00:00.000000+00:00`

## Forced-invalidated relations

| from_id | relation_type | to_id |
|---|---|---|
| `rule_forward_orchestrator_reports_to_telegram_proactively` | `applies_to` | `tech_orchestrator` |
| `peer_rollacode_bot_mac_mini_swarm_head` | `part_of` | `tech_mesh` |
| `learning_problem_agent_reports_cli_missing_persistence_layer` | `solved_by` | `fact_decision_agent_reports_persistence_model` |
| `preference_identity` | `runs_on` | `machine_blackstar` |

## Forced-invalidated relations touching doomed entities (orphan-purge case)

| from_id | relation_type | to_id |
|---|---|---|
| `tech_ha` | `applies_to` | `topic_andreys_personal_tech_stack` |
| `tech_mesh_cli` | `uses` | `tech_stats_endpoint` |

## Deleted entities (broken-endpoint case)

| id | entity_type |
|---|---|
| `tech_stats_endpoint` | `Tech` |
| `tech_ha` | `Tech` |

## Expected healing per Track

- **Track 1 (graph reads):** UI / `mem graph related` must NOT show the invalidated edges by default; `--include-invalidated` should reveal them.
- **Track 2 (graph mutations):** `mem graph revive-broken` should restore the relations whose endpoints both still exist; `mem graph purge-invalidated` should hard-delete the rest with audit.
- **Track 3 (Dreamer normalize):** Dreamer normalize phase should heal/skip relations referencing the deleted entities (cascade-death class).
- **Track 4 (Dreamer recluster):** must run end-to-end without crashing on the damaged graph.
- **Track 8/9 (evolution schema):** `dreaming-light` worker must produce a JSON report including `lessons_promoted` / `lessons_already_high_confidence`.
- **Audit:** every destructive op should show up in `graph_purge_log` / `lesson_audit`.
