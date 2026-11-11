# Team templates

Every Team starts from a template: the builtin default unless one is named. A template is an ATP package (`domios docs TEAM-FORMAT`): roles, their instructions, and what travels with them (files, memory, skills, Blueprints). Only the managers that start with the Team are created; every other role is registered and dispatched on demand through ordinary `--role`.

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
- **Consent** is the owner's: an unverified template (imported, shared by a friend) or one that asks for new grants starts only with the owner's recorded decision from its preview. A received template has zero grants and never starts by itself.
- **Payload**: files land in the Team's knowledge directory (`<data dir>/teams/<team-id>/knowledge/`) and are listed with their descriptions in the roles that use them; memory is saved to mem with the Team as its source; skills become skills of the Team's plan; automation becomes the Team's Blueprints, imported stopped under its Orchestrator (start one when the Team needs it). The owner may opt out of any item in the preview.
- A remote role runs on a paired peer Main that admits the start there; the owner consents on that peer.

`domios role list --team <team-id>` shows the role types a Team can launch and the package each comes from. A Team keeps the template version it started from: saving or importing a newer version never changes an active Team.

## Bake a Team into a template

Coordinators bake a Team whose way of working should be reused. Distil only what the Team already holds; never invent roles.

1. Read the roles: `domios role list --team <team-id>`. Every registered manager and worker is baked, including types that were never spawned.
2. Write the runbook into a pad file: how this Team works, in order, as instructions to the next orchestrator (how work is split, who checks what, when to stop). Put durable lessons there; chats and session history never travel.
3. Review without writing anything:
   ```
   domios team distill <team-id> --name <owner/slug> --version 1.0.0 --description "<what it does>" --runbook-file <pad-file> --dry-run --json
   ```
   The result holds the exact `team.yaml`, the Includes list (each item has an opt-out id) and the scrub report. Show the owner the YAML and the report; secrets appear only as names.
4. Save or export: repeat without `--dry-run`, adding `--opt-out <id>` per item the owner leaves out, `--license <spdx>` when the owner names one, and `--save` to put it in My templates. `--format yaml` (default) writes one file; a PNG icon or other binary files need `--format team` (ZIP) or `--format folder`.

The bake keeps each role's instructions, models (a role without a model stays without one), effort and start, the runbook inside the orchestrator's instructions, the Team's knowledge files, seeded memory, plan skills and Blueprints. When the Team was started from a template, `derived_from` records that template's name and version.

**The scrub report** says what left the package: machine names (placements become needs), personal and workspace paths (rewritten to the literal `{workspace}`), secret values (the name stays and becomes a secret ask), and chats/history. Prose, authors and the license are kept as written. Nothing is interpolated: `{workspace}` and `[secret NAME]` are plain text.

## Move templates

- `domios team export <name>[@version] --format yaml|team|folder [--out <path>]` writes a builtin or saved template through the same exporter.
- `domios team import <path>` checks a `.team.yaml`, a `.team` archive or a folder and saves it to My templates as unverified. A malformed package, or one that still carries a secret value, is refused before it reaches My templates. Importing another version of a saved template reports what changed; the same version with other content is refused.
- **Neighborhood**: share a `.team.yaml` or `.team` as an ordinary file attachment (at most four files, each under 2 MiB). The receiver sees a card with Preview and Dismiss; Preview checks the package and adds it to My templates as unverified. Nothing starts from the card.

Operations a build does not provide yet answer `template_unavailable` and change nothing.

<!-- revision: 4 -->
