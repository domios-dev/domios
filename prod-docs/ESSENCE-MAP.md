# Essence map

The essence map is a compact, source-backed inventory for agents reasoning about a project. It is a beacon, not an architecture blueprint: read the source essences before making structural decisions.

## Generate and validate

From a repository root, generate the map with `codeterm essence-map generate --root .`. The command derives map nodes and edges only from `essences/capabilities.md` and `essences/core.md`, then writes `essences/map.jsonl`; it never treats hand-drawn map content as authoritative. Every other root-level stable essence must carry one fenced `map` block with a non-empty `anchors:` list naming nodes from those inventories. Both generation and validation check those anchors. Validate the generated map and decision log with `codeterm essence-map validate --root .`. Generation and validation run only in the canonical checkout: a linked worktree is refused with a clear error, because a map written there is a copy, not the project's map.

## Element ids and the source grammar

Every element carries an explicit stable id authored in its source metadata. An id has the form `<type>:<slug>`, where `<type>` is `vector`, `capability`, `core`, or `client` and `<slug>` is lowercase words joined by single hyphens. An id is independent of the heading and display text: a rename keeps the id, and a genuine replacement retires the old id, names its successor in the decision recorded with `essence-map record`, and never reuses the retired id.

`essences/capabilities.md`:

- A vector heading is `## <number>. <label> {#vector:<slug>}`. The `{#...}` metadata is required.
- Each vector's capability table has the header `| ID | Capability | What it enables | Entry points | Invariants | Status | Runs on | Feeds |`. The first cell is the capability id; the second is its display label.
- `Runs on` and `Feeds` name element ids, never labels: `Runs on` lists `core:` or `client:` ids, and `Feeds` lists `capability:` ids. A cell of `—` or `-` (optionally with a parenthesised reason) means no link.

`essences/core.md`:

- A core part heading is `### <label> {#core:<slug>}`.
- Exactly one core part declares `- Root: true`; it is the project's core root. Every other core part declares `- Part of: <core-id>`.
- A client row is in a table with the header `| ID | Client | What it is | Invariants |`, the first cell being the client id. Clients are optional, so a library or offline tool with no clients is representable.

The map is JSONL. A node has `kind: "node"`, a `type` of `vector`, `capability`, `core`, or `client`, an `id`, a `label`, and a `source` containing the essence path, stable anchor, and SHA-256 digest. Capability nodes also carry their vector id and the exact source `status` cell, preserving shipped (`stable`), rollout, planned, and roadmap qualifiers. Other node types do not carry status. An edge has `kind: "edge"`, a `type` of `runs_on`, `feeds`, `part_of`, or `client_of`, plus `from`, `to`, and its source reference.

Validation rejects missing, empty, or dangling stable-essence anchors, a missing or duplicated `{#...}` id, a relation or anchor that names an unknown id, malformed records, unknown fields and kinds, missing capability statuses, dangling links, stale source digests, missing source-derived nodes, and maps over the 100-node project budget. A capability must have a runs-on edge or an explicit `runs_on_reason`; its flow may be inbound or outbound, so sink capabilities remain valid when another capability feeds them.

The declared core root is the project's own root: every core part is `part_of` it or another part, and every client is a `client_of` it. The root produces no self `part_of` edge; no repository toolchain or daemon node is added to the product map. Generated capability links are authoritative.

`essences/decisions.jsonl` is an append-only JSONL log. Each decision explains a map change with a monotonic `decision-N` id, the affected map digest, a summary, and a reason. Add a new record; do not rewrite old decisions.

## The decision journal

`codeterm essence-map record --summary "<what changed>" --reason "<why>"` appends the next decision for the map as it stands. A map whose digest already matches the last decision records nothing, so regenerating an unchanged map is silent and the log stays a record of changes rather than of runs.

Generation keeps the bytes it replaces under the CodeTerm data directory's `map-history/<project-key>/<digest>.jsonl`, content-addressed, so a recorded decision can still be restored later without adding files to the repository. `codeterm essence-map revert --to decision-N` puts that map back and appends a further decision for the revert; the log is never rewritten. The project key is derived from the canonical project root, and a map is always regenerable from its essences.

## One summary at a release

`codeterm essence-map release-summary [--since decision-N]` composes the single owner-facing report for a release: every decision after the baseline, the current map digest, and exactly two choices — keep the generated map, or revert to the baseline. The map is never announced per change; the owner hears about it once, here, and the revert choice is the one question asked.

## Who authors the map

The `cartographer` role kind is a non-anchoring Manager leaf with no Team-management, plan, release, or spawn authority. A Team may attach one optional Cartographer Manager; repeated requests reuse the same tab, and Team close removes its membership. It is offered only when `[orchestration.map] enabled` is on and the workspace root carries the source essences, and only at a natural boundary: a plan wave closing, or a plan reaching release. The boundary offer is one-shot — a `map_requested` or `map_generated` milestone on the plan disarms it, so a Cartographer is never spawned again for the same boundary.

<!-- revision: 9 -->
