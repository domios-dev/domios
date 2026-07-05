# Crate split exploration — compile-time workspace boundaries

Static analysis for task `task_a4c6b285a11d46dd97fc98a25267f285`. No new
`cargo build --timings` runs in this lane (machine load constraint); baseline
timing HTML must come from the orchestrator's warm checkout.

## Current shape

| Unit | Role | Approx. Rust LOC (src) | External deps |
|------|------|------------------------|---------------|
| `src-tauri` (`codeterm`) | Tauri daemon + lib | ~174k | tauri, tokio, axum, rquickjs, git2, … |
| `cli` (`codeterm-cli`) | Agent CLI | separate crate | clap, reqwest, rusqlite |
| `crates/mem-store` | Shared mem graph SQL | ~2k | rusqlite, serde |
| `crates/codeterm-plugins` | Plugin channel/install | ~1.5k | serde, sha2, dirs |
| `src-tauri/pty-fork` | portable-pty fork | path dep | — |

The daemon is a **single rustc unit** for almost all backend logic. Incremental
`cargo check` after a typical edit recompiles `codeterm` (and sometimes heavy
deps). Existing extractions (`mem-store`, `codeterm-plugins`) already isolate
CLI/daemon-shared code but do **not** shrink the main lib's incremental surface.

### Largest in-crate modules (proxy for compile cost)

Top `.rs` files by line count under `src-tauri/src/` (no `--timings` artifact
in repo; use orchestrator baseline):

| File | LOC | Notes |
|------|-----|-------|
| `agent/provider.rs` | 1211 | Tauri/PTY/agent trait glue |
| `mem/search.rs` | 1137 | embeddings feature, sqlite-vec |
| `plugin/js_runtime/provider.rs` | 995 | rquickjs |
| `agent/registry.rs` | 993 | provider registry |
| `lib.rs` | 988 | Tauri setup + state wiring |
| `keepawake.rs` | 988 | platform-specific |
| `service/plan/search_index.rs` | 984 | rusqlite index |
| `plan/types.rs` | 897 | **serde-only domain types** |

Critical path (expected): `codeterm` lib → largest changed module → link.
Splitting a ~900–1200 LOC pure-data crate out of `codeterm` should shorten the
**in-crate** incremental path when only that crate's sources change.

## Candidate seams (2–4)

### 1. `codeterm-plan-types` — **recommended next split**

**Scope:** `plan/types.rs` (897) + `plan/planning_state.rs` (126) +
`plan/gates.rs` (55) + `plan/ids.rs` (90) ≈ **1168 LOC**.

**Deps:** `serde`, `serde_json`, `uuid` only. No `crate::` except cross-refs
within the four files (resolved inside the new crate).

**Coupling:** ~50+ call sites use `crate::plan::{Plan, Task, …}` and
`crate::plan::ids::mint_id`; keep `src-tauri/src/plan/mod.rs` as a thin
re-export facade (`pub use codeterm_plan_types::*`) so call sites stay stable.

**Expected impact:** Medium–high on plan/CLI-heavy workstreams. Plan types
change often during Canvas/planner work; isolating them avoids recompiling
`service/plan/*`, `socket/server/plan_*`, and agent layers when only serde
fields change. Likely **>15% warm-incremental** when the touched file is inside
the extracted crate (orchestrator must confirm).

**Risks:** `Plan` embeds `PlanningState`, `PlanningTransition`, `DecisionGate`
— all must move together. `service/plan/gates.rs` (business logic) stays in
daemon; only `plan/gates.rs` (types) moves. CLI does not import these types
today (RPC JSON only) — optional CLI dep later.

### 2. `codeterm-config-schema` — deferred

**Scope:** `config/schema/mod.rs` (876 LOC), mostly serde structs + embedded
theme TOML.

**Blocker:** `UpdaterConfig.channel` uses `crate::update::Channel`. Extract
`update/channel.rs` (118 LOC, pure serde) first, or use a string newtype in
schema and convert at the daemon edge.

**Expected impact:** Medium when editing config/theme types. Touches API
routes, secrets, bridge — wide fan-out but still mostly type-only edits.

### 3. `codeterm-prompts` — prompt-edit incremental wins

**Scope:** `prompts.rs` (241) + `include_str!` from `src-tauri/prompts/*.md`.

**Deps:** none (std only).

**Expected impact:** High **when prompts change** (forces full lib rebuild today
because `include_str!` hashes into `codeterm`). Low average impact across all
edits. Good follow-on after plan-types.

**Risks:** Must keep `.md` paths stable relative to the new crate's `CARGO_MANIFEST_DIR`.

### 4. `codeterm-paths` — **prototyped on this branch**

**Scope:** former `src-tauri/src/paths.rs` (170 LOC) → `crates/codeterm-paths`.

**Deps:** `dirs` only. Zero behavior change; `src-tauri/src/paths.rs` re-exports.

**Expected impact:** **Below 15% threshold** for general edits — too small to
ship alone. Validates workspace wiring and re-export pattern before plan-types.

**Follow-up:** CLI duplicates `data_dir_name()` / `instance_suffix()` in
`cli/src/main.rs` (~80 LOC). Consolidating CLI onto `codeterm-paths` removes
drift risk but is a separate change (CLI socket heuristics differ slightly).

## Prototype on `lane/crate-split-exploration`

- Added `crates/codeterm-paths` (lib + tests moved verbatim).
- `codeterm-paths = { path = "../crates/codeterm-paths" }` in `src-tauri/Cargo.toml`.
- `src-tauri/src/paths.rs` → `pub use codeterm_paths::*;`

No logic changes. Compile not run in this lane.

## Serialized measurement protocol (orchestrator)

Run from **warm** integration checkout (`/Users/rollacode/Developer/codeterm`
or merged branch), shared target via sccache (do not use cold worktree target).

### A. Baseline timings (once per branch tip)

```bash
cd src-tauri
cargo clean   # cold only for this step
/usr/bin/time -p cargo build --timings 2>&1 | tee /tmp/codeterm-build-cold.log
open target/cargo-timings/cargo-timing.html   # note top units + critical path
```

### B. Warm incremental — paths prototype (this branch)

Compare `origin/feat/canvas-updates` vs `lane/crate-split-exploration`:

```bash
# BEFORE: checkout base, warm lib once
git checkout origin/feat/canvas-updates
cd src-tauri && cargo check -q
touch src/paths.rs
for i in 1 2 3; do /usr/bin/time -p cargo check 2>&1 | tee /tmp/paths-before-$i.log; done

# AFTER: checkout lane, warm once
git checkout lane/crate-split-exploration
cd src-tauri && cargo check -q
touch ../crates/codeterm-paths/src/lib.rs
for i in 1 2 3; do /usr/bin/time -p cargo check 2>&1 | tee /tmp/paths-after-$i.log; done
```

Record **medians** of `real` from `/usr/bin/time`. Expect modest delta; prototype
validates wiring, not ROI.

### C. Warm incremental — plan-types (if orchestrator lands next split)

```bash
touch ../crates/codeterm-plan-types/src/types.rs   # after extraction
for i in 1 2 3; do /usr/bin/time -p cargo check 2>&1 | tee /tmp/plan-types-$i.log; done
```

Compare against touching `src-tauri/src/plan/types.rs` on base — this is the
decision gate for **>15% warm-incremental** savings.

### D. Focused tests

```bash
cd src-tauri
cargo test paths:: --lib -- --nocapture
cargo test plan::types --lib -- --nocapture   # after plan-types split
cargo test -p codeterm-paths
scripts/check-file-sizes.sh
```

### E. Decision working point

Record in plan:

```text
codeterm plan record pl_dd93d764e1b94293b5e943af71dfdb5b \
  --task task_a4c6b285a11d46dd97fc98a25267f285 \
  --step "decision: paths prototype median warm check Xs→Ys; plan-types projected Z%; proceed|stop"
```

**Proceed** only if plan-types (or another seam) shows **>15% median warm
incremental** improvement or materially shrinks critical-path units in
`cargo-timing.html`. Otherwise keep exploration doc-only.

## Recommendation (pre-measurement)

1. **Merge paths prototype** if gate B is green (no compile errors, tests pass).
2. **Implement `codeterm-plan-types`** as the first ROI split (seam #1).
3. Defer config-schema until `Channel` is extracted or inverted.
4. Do not split `agent/` or `plugin/js_runtime/` yet — Tauri/PTY/rquickjs coupling
   makes boundary leaks likely.

<!-- revision: 1 -->
