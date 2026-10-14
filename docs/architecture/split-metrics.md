# Split metrics

One row per merged split, regenerated with `python3 scripts/split-metrics.py` (crates, shims, edges, tangle, app lines) plus gateway timings. `./build.sh build-history --json` gives the gateway's own durations; the `wall=` figure printed by `check-queue` is the cargo wall time without queue wait.

| date | develop SHA | crates | `pub use codeterm_*` shim lines | cross-crate path-dep edges | tangle units / non-test lines | app crate lines (incl. tests) |
|---|---|---:|---:|---:|---|---:|
| 2026-10-14 | 3d82935ad (develop ee2f09e99 merged) | 36 | 135 | 119 | 142 / 239,150 | 486,526 |
| 2026-10-14 | 0e24a11da (M0: root-aware scripts, hot-path table) | 36 | 136 | 119 | 142 / 239,051 | 486,456 |

M0: the core is still the whole app crate (`codeterm`, `src-tauri/src`); no hot area lives in its own crate yet. Both scripts take `--root PATH` and default to `src-tauri/crates/codeterm-service-core/src` when it exists, else `src-tauri/src`.

Earlier points from the roadmap: before R1 the app was 515,682 lines with a 162-unit / 266,148-line tangle; after R1-R4b 507,534 and 142 / 253,706.

## Largest crate on the hot path

Crates that recompile when one file of the area changes: the area's own crate plus every workspace crate depending on it, transitively (`cargo metadata --offline`; `python3 scripts/split-metrics.py --hot-path`). Every hot area is inside the app crate and nothing depends on it, so the largest crate is the app crate for all ten.

| hot area | own crate | crates rebuilt (own + transitive dependents) | largest crate on hot path | lines (incl. tests) |
|---|---|---:|---|---:|
| service/codex_rpc | codeterm | 1 | codeterm | 488,336 |
| service/pty_io | codeterm | 1 | codeterm | 488,336 |
| service/orchestrator | codeterm | 1 | codeterm | 488,336 |
| service/hierarchy | codeterm | 1 | codeterm | 488,336 |
| service/agent_restart | codeterm | 1 | codeterm | 488,336 |
| service/supporter_tab_state | codeterm | 1 | codeterm | 488,336 |
| api/mem_routes | codeterm | 1 | codeterm | 488,336 |
| api/hook_routes | codeterm | 1 | codeterm | 488,336 |
| socket/server | codeterm | 1 | codeterm | 488,336 |
| sentinel | codeterm | 1 | codeterm | 488,336 |

## Timing baseline (M0, single quiet-gateway batch)

Batch on 2026-10-14, develop a6672cbe1 merged, no other job on the gateway (checked via `build-history`), probe line `const _SPLIT_METRIC_PROBE: u8 = 0;` appended to one non-test file in each of the ten hot areas in one scratch commit (0948c28b5), reverted in the next commit; nothing probe-related reached develop. Times are the gateway job durations from `build-history --json` (started to ended, includes the cargo build and the run); `wall=` is the figure `check-queue` printed.

check-queue of the probe commit, run twice back to back (`errors=0 warnings=0`):

| run | wall= | cargo `Finished ... in` | reading |
|---|---:|---:|---|
| 1 | 153 s | 2m 31s | the probe touches all ten areas at once, all inside the app crate: one full re-check of the app crate after the shared check worktree was switched to this lane |
| 2 | 3 s | 1.34 s | same SHA, nothing dirty: it measures the no-op floor, not an edit |

Because the app crate is the only crate touched, this is one number for every area: the ten areas are not separable by check time today, which is the baseline the splits must beat. An edit in a single area costs the same app-crate re-check as run 1 (about 2.5 min on a re-warmed worktree; 26-46 s was seen earlier on an already-warm worktree).

test-one (`./build.sh test-one app <selector>`, job duration, first run includes the cold test-profile compile):

| area | selector | job time | result |
|---|---|---:|---|
| api/mem_routes | api::mem_routes | 11m 10s (cold test compile) | 89 passed |
| api/hook_routes | api::hook_routes | 44 s | 89 passed, 1 failed |
| socket/server | socket::server | 52 s | 360 passed, 1 failed |
| service/codex_rpc | service::codex_rpc | 32 s | 130 passed |
| service/pty_io | service::pty_io | 33 s | 268 passed, 2 failed, 12 ignored |
| service/orchestrator | service::orchestrator | 31 s | 217 passed |
| service/hierarchy | service::hierarchy | 74 s | 233 passed (test body 43 s) |
| service/agent_restart | service::agent_restart | 35 s | 170 passed |
| service/supporter_tab_state | service::supporter_tab_state | 38 s | 149 passed |
| sentinel | sentinel::scheduler | 27 s | 25 passed |

The four failing tests (hook_routes 1, socket::server 1, pty_io 2) ran on a tree whose only difference from develop is a reverted probe; their names were not captured, they are not caused by this task and need their own triage. The time of a failed run is still the real compile-plus-run time, so it is kept.

## Ranking the next splits (gain per shim, hypotheses)

| candidate | what it removes from an edit's rebuild | shims added | verdict |
|---|---|---:|---|
| API routes crate (`api/*_routes`, ~45k non-test) | route edits (api/hook_routes 62, routes 58, tab_routes 39, mem_routes 29 touches) stop re-checking the service core | 1-2 re-export lines, but `api::routes` (fan-in 53) and `api::events` (lowered) are hubs | best remaining candidate once `ServiceContext` is reachable as a trait |
| scheduler behind ports (R5 next) | sentinel runtime edits | ~3 | worth it only if `ServiceContext` becomes a trait; otherwise the scheduler stays |
| `commands` (7k, 173 touches) | tauri command edits | 1 | marginal: 21 inbound path edges |
| tiny leaves (<1k lines: `local-recording` 114, `notify-types` 110, `shutdown` 188) | seconds | 1 each | not worth it unless needed to break a cycle |
| service core clusters (codex_rpc, pty_io, orchestrator, agent_restart) | most of the hot edits | many | not splittable by path: the method-edge layer in `module-graph.md` shows `service/hierarchy` (104 pub fns, 161 caller units), `service/pty_io` (143 pub fns) and `service/supporter` as method hubs |

When a split is NOT worth it: shims or edges grow faster than check time falls. Concretely: a carve below about 2k lines, a carve whose moved code is still called through more than ~10 distinct caller units by method (see the method-edge table), or one that needs a new `pub use` per moved module instead of one per crate.
