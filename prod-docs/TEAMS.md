# Team templates

Every Team starts from a template: the builtin default unless one is named. A template is an ATP package (`domios docs TEAM-FORMAT`): one `roles` map whose every role has a `kind` (manager or worker) and its own instructions, plus what travels with them (files, memory, skills, Blueprints). A Team has exactly the roles its package lists. Only the `with_team` managers start with the Team; every other role is registered and started on demand through ordinary `--role`.

## Choose a template

Ask recall before you start a Team for a goal:

```
domios team recall "<what the owner wants done>"
```

It returns the best matches first and always the builtin default as the fallback; when nothing fits, use the default. `domios team template list` lists builtin and saved templates (My templates); `domios team template preview <name>` shows what one starts, asks for and brings. `domios team delete-template <name>[@version]` removes a saved one.

## Start a Team

```
domios workspace list
domios team pair --orchestrator <provider[:model]> --planner <provider[:model]> --workspace <workspace-id> [--template <name>]
domios send "<objective>" --tab <orchestrator-tab-id>
```

Without `--template` the standard pair starts from the default. Send the objective directly; the Team reports back to you.

- **Managers without a model** in the template are chosen at start. When the owner did not name one, the start returns `needs_choice` with the candidates; ask with `domios notify send "<question>" --level action_needed --choice "<model>"` (one `--choice` per candidate) and start again with the tapped model. Never pick for the owner.
- **Workers without a model** use the normal Worker defaults when they are dispatched.
- **The simple local form** comes first: the template, the manager models and the inputs. Where a role runs and what the template asks for (network, repo writes, shell, browser, secrets, tools, scripts) appear only when the template declares them or this machine lacks something.
- **Consent** is required only for imported or friend-shared packages, once per exact package (name, version and content hash). Builtin templates and templates saved by you or your agents on this machine need no consent; their declared permissions stay visible in the form and About > Permissions. A received template has zero grants and never starts by itself.
- **Payload**: files land in the Team's knowledge directory (`<data dir>/teams/<team-id>/knowledge/`) and are listed with their descriptions in the roles that use them; memory is saved to mem with the Team as its source; skills become skills of the Team's plan; automation becomes the Team's Blueprints, imported stopped under its Orchestrator (start one when the Team needs it). The owner may opt out of any item in the preview.
- A remote role runs on a paired peer Main that admits the start there; the owner consents on that peer (see *Peer Mains*).

- `domios team template preview <name>` shows what a template starts, asks for and brings. When it needs consent it prints a `decision_id`.
- `domios team template start <name> --orchestrator <provider[:model]> --planner <provider[:model]> [--input key=value] [--secret NAME=entry] [--opt-out item] [--decision <decision_id>]` starts it; `--team <team-id>` re-runs it on an existing Team and adds only missing registrations.

A start that cannot begin changes nothing and answers `template_start` with a typed result:
- `needs_choice`: a manager has no model. Ask the owner with `domios notify send "Choose a model" --level action_needed --choice "<harness:model>"` (one `--choice` per listed alternative), then start again with the chosen model.
- `refused`: `missing_inputs` (pass them), `consent_required` (preview, show the owner what it asks for, start with the issued `decision_id` only after they agree), `not_found` or `invalid_package`.
- `manager_failed`: a manager could not start; report the reason and choices to the owner. Nothing is retried for you.

After the Orchestrator is up, anything else that fails reaches the Orchestrator once as a summary with a line per role.

Good to know:
- Accepted package consent is remembered across Domios restarts. Starting the same imported or shared package again needs no `--decision`; a changed version or content requires fresh consent. Preview decision IDs are temporary: if you have not accepted a package before restarting, preview it again.
- A reasoning effort the chosen model does not offer runs at the nearest level it does offer instead of being refused.
- Manager models are checked before anything is created: a harness that is not installed, signed in or within quota answers `needs_choice` and no Tab or Team appears.
- A curator manager with `start: {schedule: "<min>..<max>"}` sets how often the Curator visits, kept within 45m..2h; owner Auto, Heartbeat and Off modes still decide whether it runs. A Team without a scheduled curator has no Curator rounds. A schedule on any other role is refused at preview and start with `schedule_unsupported_role`.
- Every Team starts with its Orchestrator. A Planner starts only when the template lists a `planner` manager that starts with the Team; without one the Orchestrator drafts the plan itself. Other `with_team` managers, custom ones included, join at start.
- `refused` can also be `schedule_unsupported_role` or `host_owned_role` (`watcher` and `general_agent` are Domios names, never package roles).
- `domios team check <path>` validates a package; the format is `domios docs TEAM-FORMAT`.

`domios role list --team <team-id>` shows exactly the roles a Team has, each with its kind, its Domios authority (a built-in manager's, or none) and the package it comes from; `domios role show <name> --team <team-id>` renders one. A Team with no registrations is the builtin default package as a whole. A Team keeps the template version it started from: saving or importing a newer version never changes an active Team.

## Roles at run time

- **Managers** are started by the Orchestrator with `domios agent spawn --role <name>`: built-in or custom, one live instance per name per Team (a second is refused with `team_role_already_live`). A manager is a Team member parented to the Orchestrator and never takes a task binding.
- **Workers** are dispatched with `domios plan task dispatch <plan> <task> --role <name>`. Only workers the Team lists are accepted; a manager name or an unlisted name, built-in or not, is refused with the Team's worker names. Without `--role` dispatch uses `worker` only when the Team lists it.
- **Authority** follows the built-in manager names only: `orchestrator`, `planner`, `cartographer` and `curator` of kind manager keep their Domios duties. A custom manager has none: it cannot spawn, approve or edit plans, accept or release tasks, or staff the Team. It advises, does what the Orchestrator asks, and asks the Orchestrator for anything else. Package text never grants authority.

## Peer Mains

A paired peer with Full access can host a Team or a role type. Its version comes from its own `identify`; a peer without template hosting (Domios 1.20.0 or newer) is refused up front with the version to install.

**Deploy a template on a peer** (for example "bake this Team and deploy it on Air"):

```
domios team distill <team-id> --name <owner/slug> --version 1.0.0 --description "<what it does>" --runbook-file <pad-file> --save
domios team template transfer <owner/slug> --mesh <peer> --tab <peer-tab-id>
domios team template start <owner/slug> --mesh <peer> [--orchestrator <provider[:model]>] [--planner <provider[:model]>] [--input key=value] [--secret NAME=entry]
```

- `transfer` exports a `.team` through the one exporter, carries it over the mesh attachment rail to `<peer-tab-id>` (a tab on that peer, chosen by the owner) and imports it there into My templates as unverified.
- `start --mesh` asks the peer; nothing starts until the owner taps **Start here** on that peer's action_needed notification. It returns `awaiting_owner` with a `request_id` at once; the outcome (`started`, `declined`, `refused` with a typed result, or `failed`) arrives in this tab once the owner answers. `domios team template outcome <request_id> --mesh <peer>` reads it again.
- The peer becomes the managers' home: the Team is new there, secrets bind from the peer's own vault by entry name (`--secret NAME=entry` names the peer's entry), and no Team state, plan or secret value travels. Each owner answer starts at most one Team.
- A refusal before the question (`missing_inputs`, `invalid_package`, `not_found`) comes back typed, as for a local start.

**Dispatch a role type to a peer**: `domios plan task dispatch <task> --role <type> --mesh <peer>`. The Worker is an ordinary remote member (report over mesh, `plan task merge-remote-lane`, close and restore). A peer offline before or during the run leaves an explicit state on the attempt; dispatch again elsewhere, never blindly to the same attempt.

**Missing project on the peer**: the dispatch is refused with `peer_missing_repo` and a choice of placement. When the origin's remote URL is known, the peer asks its owner "Clone <url> into <path>?" with the credentials removed from the URL; it clones only after **Clone**, then opens the checkout as a workspace. Dispatch again after that.

**Keep awake**: a peer holds itself awake while it hosts a role and lets go when the Worker's tab closes or the launch fails. Known V1 limit: Domios has one keepawake session with no owner, so if the owner starts their own keepawake while a hosted role holds one, the last release also ends the owner's session. A hosting keepawake also ends on its own after 12 hours.

## Bake a Team into a template

Coordinators bake a Team whose way of working should be reused. Distil only what the Team already holds; never invent roles.

1. Read the roles: `domios role list --team <team-id>`. Every registered role is baked with its kind and full instructions, custom managers and roles that were never spawned included.
2. Write the runbook into a pad file: how this Team works, in order, as instructions to the next orchestrator (how work is split, who checks what, when to stop). Put durable lessons there; chats and session history never travel.
3. Review without writing anything:
   ```
   domios team distill <team-id> --name <owner/slug> --version 1.0.0 --description "<what it does>" --runbook-file <pad-file> --dry-run --json
   ```
   The result holds the exact `team.yaml`, the Includes list (each item has an opt-out id) and the scrub report. Show the owner the YAML and the report; secrets appear only as names.
4. Save or export: repeat without `--dry-run`, adding `--opt-out <id>` per item the owner leaves out, `--license <spdx>` when the owner names one, and `--save` to put it in My templates. `--format yaml` (default) writes one file; a PNG icon or other binary files need `--format team` (ZIP) or `--format folder`.

The bake writes the `roles` map with `kind` first in each role and keeps each role's full instructions, models (a role without a model stays without one), effort and start, the runbook inside the orchestrator's instructions, the Team's knowledge files, seeded memory, plan skills and Blueprints. When the Team was started from a template, `derived_from` records that template's name and version.

**The scrub report** says what left the package: machine names (placements become needs), personal and workspace paths (rewritten to the literal `{workspace}`), secret values (the name stays and becomes a secret ask), and chats/history. Prose, authors and the license are kept as written. Nothing is interpolated: `{workspace}` and `[secret NAME]` are plain text.

## Move templates

- `domios team export <name>[@version] --format yaml|team|folder [--out <path>]` writes a builtin or saved template through the same exporter.
- `domios team import <path>` checks a `.team.yaml`, a `.team` archive or a folder and saves it to My templates as unverified. A malformed package, or one that still carries a secret value, is refused before it reaches My templates. Importing another version of a saved template reports what changed; the same version with other content is refused.
- **Neighborhood**: share a `.team.yaml` or `.team` as an ordinary file attachment (at most four files, each under 2 MiB). The receiver sees a card with Preview and Dismiss; Preview checks the package and adds it to My templates as unverified. Nothing starts from the card.
- **Double-click**: the installed app opens `.team` archives (not `.team.yaml`). Domios comes forward with the session dialog on that package's review in the active workspace, after the same check and save as unverified. Nothing starts until the owner accepts and presses Start.

Operations a build does not provide yet answer `template_unavailable` and change nothing.

<!-- revision: 9 -->
