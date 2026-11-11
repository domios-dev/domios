# Team templates

Every Team starts from a template: the builtin default unless one is named. `domios team pair --orchestrator <provider[:model]> --planner <provider[:model]> --template <name>` starts a Team from a template on the standard pair; without `--template` the standard pair starts. Only the managers that start with the Team are created; the other roles are registered and dispatched on demand.

Standing up a Team in a project workspace:

```
domios workspace list
domios agent providers
domios team pair --orchestrator <provider> --planner <provider> --workspace <workspace-id>
domios send "<objective>" --tab <orchestrator-tab-id>
```

`team pair` returns both tab ids and binds the Orchestrator and Planner in that workspace. Send the objective directly; the Team reports back to you.

`domios role list --team <team-id>` shows the role types the Team can launch and which package each comes from; a Team with no registrations uses the builtin default package.

- `domios team template list` lists builtin and saved templates.
- `domios team template preview <name>` shows what a template starts, asks for and brings. When it needs consent it prints a `decision_id`.
- `domios team template start <name> --orchestrator <provider[:model]> --planner <provider[:model]> [--input key=value] [--secret NAME=entry] [--opt-out item] [--decision <decision_id>]` starts it; `--team <team-id>` re-runs it on an existing Team and adds only missing registrations.

A start that cannot begin changes nothing and answers `template_start` with a typed result:
- `needs_choice`: a manager has no model. Ask the owner with `domios notify send "Choose a model" --level action_needed --choice "<harness:model>"` (one `--choice` per listed alternative), then start again with the chosen model.
- `refused`: `missing_inputs` (pass them), `consent_required` (preview, show the owner what it asks for, start with the issued `decision_id` only after they agree), `not_found` or `invalid_package`.
- `manager_failed`: a manager could not start; report the reason and choices to the owner. Nothing is retried for you.

After the Orchestrator is up, anything else that fails reaches the Orchestrator once as a summary with a line per role.
- `domios team check <path>` validates a package; the format is `domios docs TEAM-FORMAT`.
- `domios team export <name> --format team` and `domios team import <path>` move packages; `domios team distill <team-id>` bakes a live Team.

Operations a build does not provide yet answer `template_unavailable` and change nothing.

<!-- revision: 3 -->
