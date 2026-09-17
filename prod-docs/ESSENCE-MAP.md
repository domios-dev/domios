# Essence map

The essence map is a compact, source-backed inventory for agents reasoning about a project. It is a beacon, not an architecture blueprint: read the source essences before making structural decisions.

## Generate and validate

From a repository root, generate the map with `codeterm essence-map generate --root .`. The command reads `essences/capabilities.md` and `essences/core.md` and writes `essences/map.jsonl`; it never treats hand-drawn map content as authoritative. Validate both artifacts with `codeterm essence-map validate --root .`.

The map is JSONL. A node has `kind: "node"`, a `type` of `vector`, `capability`, `core`, or `client`, an `id`, a `label`, and a `source` containing the essence path, stable anchor, and SHA-256 digest. Capability nodes also carry their vector id and the exact source `status` cell, preserving shipped (`stable`), rollout, planned, and roadmap qualifiers. Other node types do not carry status. An edge has `kind: "edge"`, a `type` of `runs_on`, `feeds`, `part_of`, or `client_of`, plus `from`, `to`, and its source reference.

Validation rejects malformed records, unknown fields and kinds, missing capability statuses, dangling links, stale source digests, missing source-derived nodes, and maps over the 100-node project budget. A capability must have a runs-on edge or an explicit `runs_on_reason`; its flow may be inbound or outbound, so sink capabilities remain valid when another capability feeds them.

The service layer is the core root. Its source `Part of` statement is a containment root and produces no self `part_of` edge; no repository toolchain or daemon node is added to the product map. The five CORE dependent-capability claims that cannot be matched to capability source rows are omitted; generated capability links are authoritative.

`essences/decisions.jsonl` is an append-only JSONL log. Each decision explains a map change with a monotonic `decision-N` id, the affected map digest, a summary, and a reason. Add a new record; do not rewrite old decisions.

## The decision journal

`codeterm essence-map record --summary "<what changed>" --reason "<why>"` appends the next decision for the map as it stands. A map whose digest already matches the last decision records nothing, so regenerating an unchanged map is silent and the log stays a record of changes rather than of runs.

Generation keeps the bytes it replaces under `essences/map-history/<digest>.jsonl`, content-addressed, so a recorded decision can still be restored later. `codeterm essence-map revert --to decision-N` puts that map back and appends a further decision for the revert; the log is never rewritten. That directory is a local safety net and is not committed — a revert is taken in the checkout that generated the map, and a map is always regenerable from its essences.

## One summary at a release

`codeterm essence-map release-summary [--since decision-N]` composes the single owner-facing report for a release: every decision after the baseline, the current map digest, and exactly two choices — keep the generated map, or revert to the baseline. The map is never announced per change; the owner hears about it once, here, and the revert choice is the one question asked.

## Who authors the map

The `cartographer` role kind is a Worker leaf that owns one boundary generation. It is offered only when `[orchestration.map] enabled` is on and the workspace root carries the source essences, and only at a natural boundary: a plan wave closing, or a plan reaching release. The offer is one-shot — a `map_requested` or `map_generated` milestone on the plan disarms it, so a dispatched Cartographer is never spawned again for the same boundary.

<!-- revision: 3 -->
