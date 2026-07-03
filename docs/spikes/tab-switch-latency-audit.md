# Tab-switch latency audit

Investigation of the user-felt lock when clicking tabs, under realistic many-pane load.

## Method

Enumerated every invoke/effect fired on tab activation, mapped the backend emit path,
and reasoned about cost as a function of mounted-pane count `N`. Existing instrumentation:
`[SLOW-IPC]` (>300ms invoke) and `[LONGTASK]` (>300ms main-thread block) in
`src/lib/bridge.ts:549-591`, pushed to the `window.__ctPerf` ring buffer.

## What actually fires on a tab switch

Tab switch itself is intentionally cheap:

- `setActiveTab` (`src/store/multiplexer/index.tsx:550`) → two reducer dispatches + one
  fire-and-forget IPC `mux_set_active_tab_and_workspace`.
- Panes are **lazy-mount-once, never unmounted** (`src/App.tsx:302-310,629-673`) — inactive
  tabs are hidden via opacity/inert, not torn down. So xterm instances, chat sessions and
  parsed chat state persist; no per-pane invoke re-fires on a normal switch.
- Backend emit is coalesced (≤1/50ms) and the snapshot is built on a dedicated
  `std::thread`, off the Tauri async runtime (`src-tauri/src/service/mod.rs:220-264`).
- `plugin_manager_snapshot` (the historical 3-4s freeze) is fully resolved: in-process,
  15s cache + single-flight, `spawn_blocking`, background-refreshed channel diff
  (`src-tauri/src/commands/plugins/action.rs`, `plugins/cli.rs`).

## Root cause (residual lock, scales with N)

On **every** `mux-state-changed` apply-snapshot — which includes tab switches and
coalesced PTY output (up to 20×/s) — the store broadcasts one global
`codeterm-pane-resize` event (`src/store/multiplexer/index.tsx:219`).

Every mounted terminal pane listens for it (`src/hooks/terminal/useTerminalResize.ts`),
and the listener is gated by `resizeEnabled: terminalVisible`, where
`terminalVisible = uiMode === "terminal"` (`TerminalPane.tsx:129,160`) — i.e. **pane mode,
not tab visibility**. Because panes never unmount, all `N` mounted terminal panes across
all tabs respond. Each handler did, unconditionally:

1. `fitAddon.fit()` — a read+write on the DOM = one **forced synchronous reflow**.
2. `invoke("resize_pty", …)` — one IPC round-trip.

On a tab switch **no pane's geometry changes** (layers are always full-size), yet the cost
was `N` forced reflows + `N` resize_pty IPCs per switch. `N` fit() writes in one frame
serialize into `N` reflows (each write invalidates layout for the next read) → the
`[LONGTASK]`. The `N` IPCs flood the async runtime — the classic "cheap command resolves
last because the runtime is saturated" pattern.

This is the first slow resolver on the tab-switch path once `plugin_manager_snapshot` is
excluded; the other invokes on the path are all cache clones or one-time guarded mounts.

## Fix

Refit only when **this pane's container geometry actually changed**
(`src/hooks/terminal/paneResize.ts`, wired in `useTerminalResize.ts`). The handler now
reads `clientWidth/clientHeight`, compares to the last-fit dims, and bails early when
unchanged — the common case for tab switch and PTY-output broadcasts.

Effect per broadcast, geometry unchanged (tab switch, PTY output):

| | reflows | resize_pty IPCs |
|---|---|---|
| before | `N` (one write per pane) | `N` |
| after | `1` (read-only loop; layout served from cache after the first read, no writes) | `0` |

Real layout changes (divider drag, split add/remove, window resize) still refit, because
their dims differ from the last-fit dims. The `ResizeObserver` path records the same dims
so it and the broadcast path don't double-fit.

## Verification

- Regression test: `src/hooks/terminal/useTerminalResize.test.ts`
  (`npx tsx src/hooks/terminal/useTerminalResize.test.ts`) — asserts unchanged geometry
  does not refit, and that width/height/first-size/collapsed cases behave.
- Live repro: open many terminal panes, switch tabs, and read `window.__ctPerf` in the
  webview console (or the canvas-perf Playwright harness at
  `tests/playwright/suites/13-canvas/canvas-perf.spec.ts`, which slices the same buffer).
  Before: a `LONGTASK` and a burst of `resize_pty` per switch that grows with pane count.
  After: no `resize_pty` and no fit-driven longtask on a pure switch.

<!-- revision: 1 -->
