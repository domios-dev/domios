# Crash resilience — decouple the daemon from the WebView2 GUI (GH #183)

> Owner: Dante (orchestrator), from Andrey via BLACKSTAR 2026-06-20. ROG hard-crashed:
> a WebView2 GUI fault (HRESULT 0x8007139F) killed the single Tauri process, taking the
> headless mesh daemon (7685) down + orphaning msedgewebview2.exe. Mandate: clean
> architecture, no band-aids — the real fix is daemon/GUI separation. I own the design +
> delegate the build. ROG has a Scheduled-Task watchdog stopgap live meanwhile.
> Branch `plugins-rethink`. Tracking: GH #183.

## Root cause (from the architecture investigation)
CodeTerm is ONE OS process. `main.rs` → `lib.rs::run()` boots the WebView2 window (static
`tauri.conf.json` window) AND every headless service inside `.setup()`. The services
(REST 7685/7686, socket/IPC, mesh, cron, General Agent, mem) run on their own
`std::thread`s + Tokio runtimes (`startup/servers.rs`) — but threads share the process
address space, so a GUI fault tears the whole process down, killing the daemon. Orphaned
`msedgewebview2.exe` (WebView2's browser/GPU/renderer children) survive the dead parent
and block a clean relaunch; nothing reaps them.

## What's already in our favour (low rewrite cost)
- **The CLI↔daemon boundary is already a real cross-process protocol** — socket JSON-RPC
  (`socket/server/mod.rs`) + REST (`startup/servers.rs:117`, port 7685) + SSE/event bus.
  The CLI is a separate binary that never links the GUI (`cli/src/client.rs`). The
  server half is proven process-spanning.
- **Services already run off the Tauri runtime** on dedicated threads (`servers.rs`).
- **One chokepoint**: `ServiceContext` (`lib.rs:617`) wires everything + holds the
  `AppHandle`; `EventBroadcaster` (`startup/states.rs:67`) already exists as the in-process
  event bus, and the REST layer already mirrors events.
- **Reaper template exists**: `keepawake.rs` (journal child PIDs → inspect → `taskkill
  /T /F`). Its `inspect_process` is unix-only today; the Windows path is new code.
- **Clean-exit sentinel exists**: `crash/shutdown.rs` (`SHUTTING_DOWN` + `.clean_exit`)
  distinguishes a graceful quit from a crash — a supervisor reuses it to avoid
  relaunch-on-intentional-quit. `liveness.rs` already does out-of-band `/api/health`
  probing (template for a supervisor probe).

## Strategy — do BOTH, phased (matches the 3 asks)
The issue asks for (1) decouple, (2) startup orphan-reaper, (3) supervisor/watchdog.
Reaper + supervisor are cheap immediate HARDENING; the decouple is the DURABLE fix. Ship
the cheap wins first (they help regardless), then the decouple.

### P0 — Orphan reaper (cheap, immediate, independently valuable) — CR1
A Windows `msedgewebview2.exe` reaper modeled on `keepawake.rs`: journal the WebView2
child PIDs while alive (or enumerate by image name + dead-parent at startup via
`CreateToolhelp32Snapshot`/`Get-CimInstance Win32_Process`), and on startup
`reap_orphaned_webview2()` kills survivors before window init. Gate on the `.clean_exit`
sentinel so we never kill a healthy concurrent instance. Stops the orphan leak now,
regardless of the decouple. (keepawake's unix-only `inspect_process` needs a Win path.)

### P1 — In-app supervisor / auto-recovery — CR2
A thin supervisor that owns the process lifecycle: launch CodeTerm, probe `/api/health`
(reuse `liveness.rs`), on abnormal death reap orphans (P0) + relaunch — UNLESS
`.clean_exit` is set. On Windows this is what the installer/Start-menu launches (so it is
the parent). This SHORTENS + cleans the outage but does NOT by itself keep the daemon
alive — it is the bridge until the decouple lands, and the long-term auto-recovery for
the GUI shell after the decouple. (ROG's Scheduled-Task watchdog is the external stopgap
this replaces in-app.)

### P2+ — The decouple: standalone daemon, GUI as client (Strategy A) — the real fix
Split the headless half into its own process; the Tauri GUI becomes a thin WebView2 shell
that attaches over the existing socket/REST/event channels. GUI crash → daemon (mesh,
PTYs, agents, REST) survives untouched. Phases (each independently shippable, additive
where possible — clean rewrite of the boundary, no compat shims, no external users):
- **D1 — abstract `AppHandle` behind an `EventSink` trait.** Replace direct
  `app.emit`/`emit_everywhere` calls with an `EventSink` (impl 1: Tauri AppHandle; impl 2:
  the `EventBroadcaster` bus → SSE/WS). `ServiceContext` holds `Arc<dyn EventSink>`, not
  `AppHandle`. Additive + non-breaking; unblocks headless boot. **Do this first.**
- **D2 — headless daemon entry point.** A `--headless`/daemon mode (or a second binary)
  that boots `startup::start_servers` + state WITHOUT the window, using the bus EventSink.
  The server half already boots independently of the window once the sink is abstracted.
- **D3 — GUI-as-client.** The Tauri GUI attaches to the daemon: subscribes to the event
  bus over SSE/WS for UI updates; its ~150 `invoke` handlers become thin proxies to the
  daemon over socket/REST (much already mirrored in REST — audit coverage parity).
- **D4 — PTY ownership in the daemon.** PTYs live in the daemon (survive GUI crash); GUI
  renders xterm from streamed output (already the backend-reader model,
  `service/pty_lifecycle.rs`). Move resize/focus ownership across the boundary.
- **D5 — launch/attach lifecycle + packaging.** GUI discovers/attaches to the daemon
  (health-check 7685, start if absent); supervisor (P1) owns the daemon's lifecycle;
  `build.sh`/installer ship + start the daemon binary.

## Tasks
- **CR1 (Rust, Windows) — orphan reaper** [P0, dispatch now]. New `webview2_reap.rs`
  (or extend keepawake): journal/enumerate msedgewebview2 children, `reap_orphaned_webview2()`
  at startup, sentinel-gated. TDD the journal+decision logic; the Windows kill path mirrors
  keepawake `terminate_pid`. ROG verifies the real orphan-reap on Windows.
- **CR2 (Rust) — supervisor/watchdog** [P1]. Process supervisor: launch + health-probe +
  sentinel-gated relaunch + reap-on-death. Cross-platform; Windows = installer entry point.
- **D1 (Rust) — EventSink abstraction** [P2, the keystone of the decouple]. Additive.
- **D2–D5** — staged after D1, per above. Each its own worker, gated, parity-checked.

## Guardrails
- Reaper must NEVER kill a healthy concurrent CodeTerm's webview2 — gate on PID-journal
  ownership + the `.clean_exit` sentinel; match exact image name + verified dead parent.
- Supervisor must honor `.clean_exit`/`SHUTTING_DOWN` — no relaunch on intentional quit.
- The decouple is a clean boundary rewrite (no external users) — no compat shims; but
  each phase must keep the app working (D1 additive; D2 adds a mode; D3/D4 flip the GUI to
  client only when parity is proven). Daemon NEVER shells out to its own CLI (AGENTS.md).
- Cross-platform: macOS/Windows/Linux. Windows is where the crash + orphans live — ROG
  verifies P0/P1 and the decouple on Windows.

<!-- revision: 1 -->
