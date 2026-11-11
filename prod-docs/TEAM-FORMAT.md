# Team package format (ATP)

A team template is one Agentic Team Protocol (ATP) package. Validate one with `domios team check <path>`; it accepts all three forms and prints `file:line:column: severity[code] path: message` per finding.

## Three forms

- `name.team.yaml`: one file with everything inline. Every reference must resolve inline (`files[].content`, inline skills and automation, inline `instructions`).
- `name.team`: a ZIP with `team.yaml` at its root plus only the files `team.yaml` references.
- A folder with the same layout as the ZIP.

Archive and folder rules: exactly one root `team.yaml`; an unreferenced entry is an error; paths are relative with forward slashes (no `..`, no absolute paths, no links); size and count bounds apply; entries are written `team.yaml` first, then by path, stored with fixed metadata, so export, import and export again is byte-identical. Binary files (a PNG icon, images) need the ZIP or folder form.

## Top level

Write blocks in this order; readers accept any order.

1. Identity: `name` (`<owner>/<slug>`, owner `[a-z0-9.-]+`, slug `[a-z0-9-]+`), `version` (semantic version), `title`, `description`, `icon` (package path to an SVG or PNG), `authors` (`{name, email?, url?, role?: author|maintainer|contributor}`), `links` (`{homepage?, repository?, issues?, docs?}`), `license`, `when` (phrases for recall), `derived_from` (`{name, version}`).
2. Start asks: `project` (`required` or `none`, default `none`), `requires.harnesses`, `inputs`, `permissions`, `secrets`.
3. Payload: `files`, `memory`, `skills`, `automation`.
4. `managers`, then `workers`.
5. `x-*` vendor extensions, opaque to ATP. They are also allowed inside role objects.

Required: `name`, `version`, `description` and `managers.orchestrator`. Every other key is strictly typed; an unknown key is an error. There are no top-level `instructions`, `instructions_file` or `brief`, and no plan, spec or task templates.

- `inputs`: `key: description`, or `{description, required?, default?, choices?}`. `required` defaults to true; a default must be one of the choices.
- `permissions`: `network` (`true`, `false` or a host list with wildcards), `repo_write`, `shell`, `browser`.
- `secrets`: `{name, purpose, required?}` with `UPPER_SNAKE_CASE` names; `required` defaults to true. Values never travel.
- `files`: `{path, description?, content?}`; knowledge documents given to roles.
- `memory`: `{title, kind: fact|rule|learning, body, tags?}`.
- `skills`: objects only, `{path}` (a folder whose `SKILL.md` frontmatter must carry `name`) or inline `{name, description, content, files?}`.
- `automation`: `{path, description?}` or inline `{name, description?, content}`; Domios reads Blueprints.

## Roles

`managers` has fixed slots: `orchestrator` (required), `planner`, `cartographer`, `curator`. `workers` have free names matching `[a-z][a-z0-9_]*`; a manager slot name is not a worker name. Each role has the same fields:

- `name` (display name), `description`;
- `start`: `with_team`, `on_demand` or `{schedule: "<min>..<max>"}` with positive `m` or `h` durations and min <= max. Defaults: orchestrator and planner `with_team`, cartographer `on_demand`, curator `{schedule: 45m..2h}`, workers `on_demand`. The orchestrator always starts with the Team.
- `model`: an ordered `<harness>:<model>` list, `{same_as: <role>}` naming any role in the package (no cycles), or omitted;
- `reasoning`, `skills` (names of package skills), `tools` (`mcp:<registry name>`), `files` (paths declared in top-level `files`);
- `instructions` or `instructions_file`.

A role named like a manager slot or a host builtin worker takes the host default for every omitted field. A new worker type needs `instructions` or `instructions_file`; Domios builtin workers are `worker`, `qa`, `reviewer` and `release_manager`. Removed fields (`kind`, `persona`, `extends`, `single`, `includes`, `intent`, `needs`, `model: inherit`) are errors.

## YAML rules

YAML 1.2, one document. No anchors, aliases, tags or duplicate keys; tabs never indent; CRLF is read as LF; NUL is refused. Text has no interpolation. The canonical writer uses literal blocks (`|`, `|-`, `|+`, with an indentation indicator when the first line starts with whitespace) for multi-line text, flow lists for short single-line lists, and double quotes for anything plain style would change.

## Findings

Errors make a package invalid; warnings do not. A skill a role names but the package lacks is the warning `unresolved_skill`, and the role starts without it. Host gaps such as a missing harness, MCP tool or secret binding are shown at start, not reported by the format check. Every finding carries a stable code, for example `removed_field`, `unknown_field`, `missing_field`, `missing_file`, `unreferenced_entry`, `path_escape`, `linked_entry`, `unknown_same_as`, `same_as_cycle`, `icon_not_image`, `svg_script`, `yaml_alias` or `tab_indentation`.

<!-- revision: 1 -->
