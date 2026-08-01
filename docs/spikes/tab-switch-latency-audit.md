# Tab-switch latency audit

Investigation of the user-felt lock when clicking tabs, under realistic many-tab load.

## Method

Enumerated every invoke/effect fired on tab activation, mapped the backend emit path,
and reasoned about cost as a function of mounted-tab count `N`. Existing instrumentation:
`[SLOW-IPC]` (>300ms invoke) and `[LONGTASK]` (>300ms main-thread block) in
`src/lib/bridge.ts:549-591`, pushed to the `window.__ctPerf` ring buffer.

## What actually fires on a tab switch

Tab switch itself is intentionally cheap:

- `setActiveTab` (`src/store/multiplexer/index.tsx:550`) → two reducer dispatches + one
  fire-and-forget IPC `mux_set_active_tab_and_workspace`.
- Tabs are **lazy-mount-once, never unmounted** (`src/App.tsx:302-310,629-673`) — inactive
  tabs are hidden via opacity/inert, not torn down. So xterm instances, chat sessions and
  parsed chat state persist; no per-tab invoke re-fires on a normal switch.
- Backend emit is coalesced (≤1/50ms) and the snapshot is built on a dedicated
  `std::thread`, off the Tauri async runtime (`src-tauri/src/service/mod.rs:220-264`).
- `plugin_manager_snapshot` (the historical 3-4s freeze) is fully resolved: in-process,
  15s cache + single-flight, `spawn_blocking`, background-refreshed channel diff
  (`src-tauri/src/commands/plugins/action.rs`, `plugins/cli.rs`).

## Root cause (residual lock, scales with N)

On **every** `mux-state-changed` apply-snapshot — which includes tab switches and
coalesced PTY output (up to 20×/s) — the store broadcasts one global
`codeterm-tab-resize` event (`src/store/multiplexer/index.tsx:219`).

Every mounted terminal Tab listens for it (`src/hooks/terminal/useTerminalResize.ts`),
and the listener is gated by `resizeEnabled: terminalVisible`, where
`terminalVisible = uiMode === "terminal"` (`TerminalTab.tsx:129,160`) — i.e. **Tab mode,
not tab visibility**. Because Tabs never unmount, all `N` mounted terminal Tabs across
all tabs respond. Each handler did, unconditionally:

1. `fitAddon.fit()` — a read+write on the DOM = one **forced synchronous reflow**.
2. `invoke("resize_pty", …)` — one IPC round-trip.

On a tab switch **no Tab's geometry changes** (layers are always full-size), yet the cost
was `N` forced reflows + `N` resize_pty IPCs per switch. `N` fit() writes in one frame
serialize into `N` reflows (each write invalidates layout for the next read) → the
`[LONGTASK]`. The `N` IPCs flood the async runtime — the classic "cheap command resolves
last because the runtime is saturated" pattern.

This is the first slow resolver on the tab-switch path once `plugin_manager_snapshot` is
excluded; the other invokes on the path are all cache clones or one-time guarded mounts.

## Fix

Refit only when **this Tab's container geometry actually changed**
(`src/hooks/terminal/tabResize.ts`, wired in `useTerminalResize.ts`). The handler now
reads `clientWidth/clientHeight`, compares to the last-fit dims, and bails early when
unchanged — the common case for tab switch and PTY-output broadcasts.

Effect per broadcast, geometry unchanged (tab switch, PTY output):

| | reflows | resize_pty IPCs |
|---|---|---|
| before | `N` (one write per Tab) | `N` |
| after | `1` (read-only loop; layout served from cache after the first read, no writes) | `0` |

Real layout changes (divider drag, split add/remove, window resize) still refit, because
their dims differ from the last-fit dims. The `ResizeObserver` path records the same dims
so it and the broadcast path don't double-fit.

## Verification

- Regression test: `src/hooks/terminal/useTerminalResize.test.ts`
  (`npx tsx src/hooks/terminal/useTerminalResize.test.ts`) — asserts unchanged geometry
  does not refit, and that width/height/first-size/collapsed cases behave.
- Live repro: open many terminal Tabs, switch tabs, and read `window.__ctPerf` in the
  webview console (or the canvas-perf Playwright harness at
  `tests/playwright/suites/13-canvas/canvas-perf.spec.ts`, which slices the same buffer).
  Before: a `LONGTASK` and a burst of `resize_pty` per switch that grows with Tab count.
  After: no `resize_pty` and no fit-driven longtask on a pure switch.

## Round 2 — click-to-paint measurement (task_fd79beb6)

Built a dedicated latency oracle rather than relying on inference from the resolver
audit above: `tests/playwright/suites/13-canvas/tab-switch-latency.spec.ts` drives a
**warm** session (every tab visited once so it's in `mountedTabIds`, matching the
mount-once-keep-alive model) with a real workspace containing 20 terminal tabs plus a
live plan, and toggles the Mission Control workspace-canvas overlay
(`CanvasButton`/`data-testid="canvas-toolbar-button"`) as the plan/canvas surface. It
measures every click with the browser's native **Event Timing API**
(`PerformanceObserver({type:"event"})`) — an interaction entry's `duration` already
spans input delay + processing + presentation (paint), the same signal behind the
Interaction-to-Next-Paint web vital, so it is a faithful click-to-first-paint number
rather than a hand-rolled `requestAnimationFrame` proxy.

Result on eight's dev (post round-1 fix, N=20 mounted Tabs + 1 plan overlay, 24
tab-switch clicks + 6-12 canvas-toggle clicks per run, repeated at N=12/18/20/30 for a
scaling check):

| Tabs | tab-switch p50 | tab-switch p95/max | canvas-toggle p50/p95/max | LONGTASK/SLOW-IPC |
|---|---|---|---|---|
| 12 | 32ms | 40ms | 16ms | none |
| 18 | 32ms | 32ms | 16ms | none |
| 20 | 32ms | 32ms | 16ms | none |
| 30 | 24ms | 40ms | 16ms | none |

Flat across 12→30 mounted Tabs, comfortably under the 150ms budget, with zero
`[SLOW-IPC]`/`[LONGTASK]` samples in any run. Round 1's O(N)→O(1) resize/refit fix was
the only resolver on this path (per the root-cause analysis above); this round confirms
empirically that fixing it was sufficient — there is no further bottleneck to chase on
the render/mount path at this scale. `SidebarWorkspaceTabs`/`Sidebar` are unmemoized
(full re-render on every `mux-state-changed`) but this did not show up as measurable
cost, because `TabItem`'s custom comparator (`tabItemPropsEqual`,
`src/components/Sidebar/TabItem.tsx:336`) already bails the two changed rows' children
from re-rendering; flagged here as a future micro-opt candidate only if a heavier
Sidebar payload (many workspaces, deep hierarchy trees) is ever shown to move the
number.

Incidentally hardened `tests/playwright/helpers/dev-console.ts`'s `waitForShell`: a
fresh Playwright context has no prior service-worker controller, so
`src/lib/serviceWorkerRegistration.ts`'s `controllerchange` → `location.reload()` can
fire moments after `#root` attaches, destroying the harness's own execution context.
This pre-existing race blocked every web-mode UI test (not just this one) after a dev
restart; `waitForShell` now retries its own sequence once on that specific error.

Verification: `CT_E2E_PORT=7686 npx playwright test -c tests/playwright -g "tab-switch-latency" --workers=1 --project=full`.

<!-- revision: 3 -->
