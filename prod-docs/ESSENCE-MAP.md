# Essence map

The essence map is a compact, source-backed inventory for agents reasoning about a project. It is a beacon, not an architecture blueprint: read the source essences before making structural decisions.

## Generate and validate

From a repository root, generate the map with `codeterm essence-map generate --root .`. The command reads `essences/CAPABILITIES.md` and `essences/CORE.md` and writes `essences/map.jsonl`; it never treats hand-drawn map content as authoritative. Validate both artifacts with `codeterm essence-map validate --root .`.

The map is JSONL. A node has `kind: "node"`, a `type` of `vector`, `capability`, `core`, or `client`, an `id`, a `label`, and a `source` containing the essence path, stable anchor, and SHA-256 digest. Capability nodes also carry their vector id. An edge has `kind: "edge"`, a `type` of `runs_on`, `feeds`, `part_of`, or `client_of`, plus `from`, `to`, and its source reference.

Validation rejects malformed records, unknown fields and kinds, dangling links, stale source digests, missing source-derived nodes, and maps over the 100-node project budget. A capability must have a runs-on edge or an explicit `runs_on_reason`; its flow may be inbound or outbound, so sink capabilities remain valid when another capability feeds them.

The service layer is the core root. Its source `Part of` statement is retained as a self `part_of` marker; no repository toolchain or daemon node is added to the product map.

`essences/decisions.jsonl` is an append-only JSONL log. Each decision explains a map change with a monotonic `decision-N` id, the affected map digest, a summary, and a reason. Add a new record; do not rewrite old decisions.

<!-- revision: 2 -->
