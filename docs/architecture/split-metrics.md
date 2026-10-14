# Split metrics

One row per merged split, regenerated with `python3 scripts/split-metrics.py` (crates, shims, edges, tangle, app lines) plus gateway timings. `./build.sh build-history --json` gives the gateway's own durations; the `wall=` figure printed by `check-queue` is the cargo wall time without queue wait.

| date | develop SHA | crates | `pub use codeterm_*` shim lines | cross-crate path-dep edges | tangle units / non-test lines | app crate lines (incl. tests) |
|---|---|---:|---:|---:|---|---:|
| 2026-10-14 | 3d82935ad (develop ee2f09e99 merged) | 36 | 135 | 119 | 142 / 239,150 | 486,526 |

Earlier points from the roadmap: before R1 the app was 515,682 lines with a 162-unit / 266,148-line tangle; after R1-R4b 507,534 and 142 / 253,706.

## Timing baseline (partial, not yet reliable)

`check-queue` of a lane SHA after a one-line private-const append, shared warm check worktree, `errors=0`, `wall` seconds:

| area | touch | run 1 | run 2 |
|---|---|---:|---:|
| sentinel (codeterm-sentinel) | `probe.rs` | 162 (worktree re-warm after other lanes) | 46 |
| wire (codeterm-wire) | `domios.rs` | 10 | 45 |

Reading: the shared check worktree is switched by every lane, so run 1 is dominated by what the previous job left behind (10 s to 162 s for similar edits). Only back-to-back runs on a quiet gateway are comparable; the 45-46 s figure is the realistic warm cost of an edit in either crate today, and it is nearly the same for a leaf crate (wire) and for the hottest split crate (sentinel) because the app crate, which depends on both, is re-checked in full each time. Config, the app core, codex_rpc, pty_io, orchestrator, api/mem_routes, socket/server and the `test-one` times are not measured: the runs were stopped when the gateway was handed back for its fix. They are to be taken in one uninterrupted batch once the gateway is declared fixed.

Largest crate on the hot edit path: `codeterm-plugin` (72k lines, 849 touches in 90 days), then `codeterm-plan` (63k) and `codeterm-pty` (30k). The app crate (486k lines) is on every path.

## Ranking the next splits (gain per shim, hypotheses)

| candidate | what it removes from an edit's rebuild | shims added | verdict |
|---|---|---:|---|
| API routes crate (`api/*_routes`, ~45k non-test) | route edits (api/hook_routes 62, routes 58, tab_routes 39, mem_routes 29 touches) stop re-checking the service core | 1-2 re-export lines, but `api::routes` (fan-in 53) and `api::events` (lowered) are hubs | best remaining candidate once `ServiceContext` is reachable as a trait |
| scheduler behind ports (R5 next) | sentinel runtime edits | ~3 | worth it only if `ServiceContext` becomes a trait; otherwise the scheduler stays |
| `commands` (7k, 173 touches) | tauri command edits | 1 | marginal: 21 inbound path edges |
| tiny leaves (<1k lines: `local-recording` 114, `notify-types` 110, `shutdown` 188) | seconds | 1 each | not worth it unless needed to break a cycle |
| service core clusters (codex_rpc, pty_io, orchestrator, agent_restart) | most of the hot edits | many | not splittable by path: the method-edge layer in `module-graph.md` shows `service/hierarchy` (104 pub fns, 161 caller units), `service/pty_io` (143 pub fns) and `service/supporter` as method hubs |

When a split is NOT worth it: shims or edges grow faster than check time falls. Concretely: a carve below about 2k lines, a carve whose moved code is still called through more than ~10 distinct caller units by method (see the method-edge table), or one that needs a new `pub use` per moved module instead of one per crate.
