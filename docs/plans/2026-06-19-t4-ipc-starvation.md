# T4 root cause — async-runtime starvation freezes the whole UI (3-10s)

## Symptom
Switching / opening / closing a tab freezes the ENTIRE interface (all panes, sidebar,
everything) for 3-4s, up to ~10s. Happens with chat UI OFF, on plain terminals,
worst on first-open-after-restart and with many panes.

## Diagnosis (measured, not guessed)
Frontend IPC instrumentation (`src/lib/bridge.ts`: log any `invoke` > 300ms + a
`longtask` observer; webview console is piped into `/tmp/codeterm-dev.log` via vite)
captured, during one tab switch:
```
get_config              4041ms   ← trivial command, does no heavy work
plugin_manager_snapshot 2652ms
list_agent_providers    2715ms
get_pane_history_raw    2657ms   (× MANY panes: 9d0d9ef8, 6274bdd4, 22490f40, 2a266305, 39e26550…)
claim_resize_owner      2646ms
read_pty                9874ms   ← red herring: legitimate long-poll (rx.recv().await)
```
SMOKING GUN: unrelated commands block for the SAME duration and resolve at the SAME
instant. A command that does nothing (`get_config`) blocking 4s ⇒ it is a VICTIM
queued behind something holding the whole IPC channel.

ROOT: Tauri command handlers are `async fn` but call the SYNCHRONOUS
`dispatch::dispatch` (`src-tauri/src/dispatch/mod.rs:113`) inline on the tokio async
worker threads. `get_pane_history_raw` (`commands/pane.rs:31`) →
`Command::GetOutput { full: true }` → reads a pane's ENTIRE scrollback + serializes to
String, ON an async worker. On tab switch / first-mount MANY panes lazily mount at
once → many `get_pane_history_raw` fire concurrently → every async worker is blocked
in synchronous buffer work → the runtime is starved → even `get_config` cannot be
polled → the whole UI hangs until the wave drains.

This is the classic Tauri antipattern: **blocking/synchronous work on the async
runtime**. The `dispatch` layer is inherently synchronous (locks + buffer ops); calling
it directly from `async fn` commands lets any heavy command starve all IPC.

## Fix
1. **Take blocking dispatch off the async runtime.** Heavy `dispatch::dispatch`-based
   commands (`get_pane_history_raw`, `get_pane_text`, and any GetOutput/GetText/heavy
   variant) run via `tokio::task::spawn_blocking` (ServiceState is `Arc<ServiceContext>`
   → clone the Arc into the closure). The async worker yields; the blocking pool (512
   threads) absorbs the reads; trivial commands like `get_config` stay responsive →
   no full-UI freeze even while a pane replays.
2. **Bound the replay.** Full unbounded scrollback replay per pane is wasteful (xterm
   scrollback is bounded anyway). Cap the first-mount replay (last N KB / lines) instead
   of the entire buffer — faster, less work, and stagger/defer so N panes don't all
   replay in the same frame.

## UPDATE — the dominant root is the plugin VM lock held across blocking network I/O
Empty tabs (0 messages, empty scrollback) STILL lag 2-6s ⇒ it is NOT data processing,
it is **lock contention**. Confirmed in code:
- `PluginVm { inner: Mutex<Inner> }` (`plugin/js_runtime/vm.rs:161`); `call_json`
  (`vm.rs:384`) holds `vm.inner.lock()` (`vm.rs:407`) for the WHOLE call.
- `host.fetch` (`host_api.rs:491`) uses `reqwest::blocking` with a **30s** default
  timeout (`host_net.rs:155`) and builds a NEW client per call (no keep-alive — 243
  fresh connections to the remote LM Studio in one log window).
- The lmstudio plugin polls a REMOTE LM Studio (eight, 192.168.1.126). Each `poll`/
  `listModels`/`sessionInfo` holds the single plugin VM mutex across the blocking
  network round-trip. Meanwhile UI-facing plugin reads on tab switch (provider list,
  `chatbackend_session_info`, `plugin_manager_snapshot`) block on `vm.inner.lock()` →
  those run on the Tauri async runtime → starvation → the ENTIRE UI freezes 2-6s,
  even on empty plain-terminal tabs.

This is the lock to fix. The earlier dispatch/spawn_blocking fix was real but secondary.

### Fix (real cure)
- **`host.fetch` must NOT hold the VM lock during network.** Mirror the async
  `host.exec` job-id+poll pattern: kick the blocking request onto a background thread,
  return a job id immediately (releasing `vm.inner`), JS polls for the result. The VM
  mutex is then free during the network wait.
- **Decouple UI plugin reads from the poll loop:** cache `listModels` / `sessionInfo` /
  the spawnable-provider descriptors so `+new session`, the providers list, and the
  plugin snapshot never call into the VM on the hot path (they read cache; the poll
  loop refreshes it).
- **Hygiene:** reuse a keep-alive `reqwest` client; drop the default fetch timeout
  30s → a few seconds; keep idle poll backoff.

## Prevention (so this class never ships again)
- **Architectural rule (docs/contributing/architecture.md + style.md):** synchronous or
  blocking work (lock held across heavy op, disk, large serialization) MUST NOT run on
  the async runtime. A Tauri `async fn` command that calls `dispatch::dispatch` (or any
  blocking op) MUST wrap it in `spawn_blocking` — or be a sync `fn` (Tauri off-loads sync
  commands). Branch on capability, not vibes: "does this command block? → spawn_blocking."
- **CI guard** (mirror `scripts/check-mux-write.sh`): `scripts/check-blocking-in-async.sh`
  greps for `async fn` Tauri commands whose body calls `dispatch::dispatch(` directly
  (not through the blocking helper) and fails CI. One canonical helper
  `dispatch_blocking(cmd, svc).await` so the guard has a single allowed call shape.
- **Bounded by default:** any "read full history / full buffer" path is capped; unbounded
  reads are a review red flag.

## Early detection (catch it the moment it regresses, in dev)
- **Keep the perf instrumentation as a first-class dev tool.** The IPC-slow logger +
  `longtask` observer in `bridge.ts` is exactly what caught this. Keep it (dev-gated),
  surface `window.__ctPerf`, and pipe slow samples to the dev log. A 4s `get_config` in
  the log is an instant "something is starving the runtime" tripwire.
- **Backend budget log:** instrument `dispatch::dispatch` to `log::warn` when a single
  dispatch exceeds a budget (e.g. 100ms) with the `Command` variant — names the culprit
  directly, no frontend correlation needed.
- **Perf smoke test (perf regression gate):** an automated scenario that restores/opens
  N tabs (realistic: many panes, long scrollback) and asserts NO IPC exceeds a threshold.
  Add under `evals/` or a Playwright perf test. The bug only manifests at scale — test at
  scale, not on a clean 2-tab dev setup.
- **Profile under realistic load**, not the empty dev instance.

<!-- revision: 1 -->
