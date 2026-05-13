# Frontend Code Review — Domain 12
<!-- revision: 1 -->

**Date:** 2026-05-13
**Scope:** All `.ts` / `.tsx` files under `src/` (144 files total, excluding test files and generated code)
**Reviewer:** Claude Sonnet 4.6 (automated)
**Categories:** Correctness · Architecture · Consistency

---

## Summary Table

| # | Severity | File | Finding |
|---|----------|------|---------|
| 1 | CRITICAL | `SecretsTab.tsx` | `lock()` and `logout()` have no try/catch around `await fetch()` — unhandled rejection crashes silently |
| 2 | CRITICAL | `SecretsTab.tsx` | Mock backend shortcut calls `submitPasswordLogin` immediately after `setMasterPassword` — React state is async, the form closure still holds `""` |
| 3 | HIGH | `ChatView.tsx` | Stale closure: `handleKeyDown` dep array lists `[sendMessage]` but the handler calls `toggleExpand` which is not included |
| 4 | HIGH | `SchemaRenderer.tsx` | `key={i}` index used for dynamically-ordered plugin schema fields at lines 44, 83, 215 — reorder causes identity loss |
| 5 | HIGH | `MemoryPanel.tsx` | Dynamic `import('@tauri-apps/api/event')` inside `useEffect` with no `.catch()`, inner `.listen()` calls also uncaught |
| 6 | HIGH | `MeshPanel.tsx` | Receives props `api`, `config`, `onConfigUpdate` in interface but component only destructures `onClose` — props silently unused |
| 7 | HIGH | `BrowserPane.tsx` | Hardcodes prod port `7685` in `resolveUrl` — breaks in dev environment and on non-default port config |
| 8 | MEDIUM | `useShellIntegration.ts` | `console.log` at line 63 in the hot path — fires every time a pending command is sent to a new pane |
| 9 | MEDIUM | `ChatView.tsx` | `isLoading` / `_setIsLoading` dead state — `useState(false)` where setter is never called, always false |
| 10 | MEDIUM | `ChatView.tsx` | `invoke<{ panes: Record<string, { pty_id: string | null }> }>("mux_get_state")` uses ad-hoc inline type instead of existing `MuxSnapshot` interface |
| 11 | MEDIUM | `Sidebar.tsx` | `collectPaneIdsLocal` at line 264 re-implements `collectPaneIds` from `store/multiplexer/tree.ts` — DRY violation |
| 12 | MEDIUM | `Sidebar.tsx` | `renderTab` is a plain function defined inside the render body, not a component — recreated every render, blocks list virtualization |
| 13 | MEDIUM | `store/multiplexer/index.tsx` | `closeWorkspace`, `closePane`, `closeTab` callbacks list `[state]` in deps — recreated on every state change, causing listener re-registration cascade |
| 14 | MEDIUM | `MemoryActivityTab.tsx` | `autoScroll` in the dep array of the listen `useEffect` — listener torn down and re-registered every scroll toggle; event gap possible |
| 15 | MEDIUM | `MeshPanel/NetworkTab.tsx` | `console.error` for "Forget peer failed" and "Leave mesh failed" — user sees no feedback, error is only in DevTools |
| 16 | MEDIUM | Multiple files | `alert()` used for error reporting in 10 places: `ServiceDrawer`, `CronDrawerSection`, `BroadcastToggle`, `MemoryActivityTab` |
| 17 | LOW | `BroadcastToggle.tsx` | `alert()` on broadcast toggle failure — blocks UI thread |
| 18 | LOW | Multiple files | 25+ `console.warn` / `console.error` calls throughout production code — no structured error surface |
| 19 | LOW | `useXterm.ts`, `usePtyConnection.ts` | `navigator.platform` usage (deprecated API) for OS detection; should use `navigator.userAgent` or a Tauri-provided flag |
| 20 | LOW | `Swarm/SwarmOverlay.tsx` | `// eslint-disable-line react-hooks/exhaustive-deps` at visibility transition — `animState` missing from deps; benign but note-worthy |
| 21 | LOW | `AgentRunLogPopup.tsx` | `key={i}` for event rows — acceptable (events are append-only stream) but fragile if replay re-orders |
| 22 | LOW | `EventRenderer.tsx` | `key={i}` for content blocks — stable within a message but could become stale if merging logic changes |
| 23 | LOW | `DropdownMenu.tsx` | `key={i}` for menu items — items come from `useMemo`; acceptable but any future dynamic menu would break |
| 24 | LOW | `InternalToast.tsx` | `dismissed` is a `Set` held in state; the `useEffect` for auto-dismiss reads it as a dep, causing extra renders on dismiss |
| 25 | LOW | `useUpdate.ts` | `mountedRef` pattern to skip StrictMode double-mount — prevents the first-load silent check from running twice, but also silently skips if the component remounts legitimately |
| 26 | LOW | `Sidebar.tsx` | Inline agent spawn modal (~50 lines) inside the 663-line Sidebar — should be its own component |
| 27 | LOW | `useContributions.ts` | Multi-line doc comment block (`/**…*/`) on module-level hook — violates one-line comment rule |
| 28 | LOW | `KeepAwakeTray.tsx` | `console.error("[keepawake]", e)` logged but user-visible error state not shown; user cannot tell start failed |

---

## Correctness

### CRITICAL — `SecretsTab.tsx`: Unhandled rejections in `lock()` and `logout()`

Lines 363–375 contain two `async` callbacks that `await fetch(...)` with no `try/catch`:

```ts
const lock = async () => {
  await fetch(`${base}/api/secrets/lock`, { method: "POST", headers });
  // ... update state
};
const logout = async () => {
  await fetch(`${base}/api/secrets/logout`, { method: "POST", headers });
  // ... update state
};
```

If the fetch rejects (network error, daemon down, port mismatch), the promise goes unhandled. In Tauri's WebView the error is swallowed silently — no toast, no UI feedback. Both operations have side effects (clearing credentials), so a silent failure leaves the UI in an inconsistent state.

**Fix:** wrap each with `try/catch`, surface the error via existing `setError` state or the notification system.

---

### CRITICAL — `SecretsTab.tsx`: React async state race in mock backend shortcut

Line 662–669 does:

```ts
setMasterPassword("dev-only");
submitPasswordLogin(new Event("submit") as unknown as React.FormEvent);
```

`setMasterPassword` queues a state update; `submitPasswordLogin` runs synchronously in the current event loop tick, where the closure still captures the old `masterPassword` value (`""`). The form submission fires with an empty password.

Additionally, `new Event("submit") as unknown as React.FormEvent` is a double-cast that bypasses TypeScript's type system. The function accesses `e.preventDefault()` on the real event; calling it on a native `Event` instead of a React synthetic event is harmless here, but the pattern is fragile.

**Fix:** use `setMasterPassword` then trigger submit in a `useEffect` that watches for the new value, or pass the password directly as a parameter.

---

### HIGH — `ChatView.tsx`: Stale closure in `handleKeyDown`

```ts
const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
  // ...
  if (e.key === "Escape") toggleExpand();   // toggleExpand called here
  // ...
}, [sendMessage]);                           // toggleExpand NOT in deps
```

`toggleExpand` is defined above (line ~330) and depends on `expanded` state. `handleKeyDown` captures the version of `toggleExpand` from the first render and never updates. After a collapse/expand, `Escape` operates on stale state.

**Fix:** add `toggleExpand` to the `useCallback` dep array.

---

### HIGH — `SchemaRenderer.tsx`: Index keys for dynamic plugin fields

At lines 44, 83, and 215, schema fields from plugin-contributed `notifications_schema` arrays are rendered with `key={i}` (the array index). If a plugin update reorders fields, React reuses the wrong DOM node, causing lost input values and missed re-renders.

**Fix:** use `field.key` or a stable slug derived from the field name — plugin schema fields have a `key` property already.

---

### HIGH — `MemoryPanel.tsx`: Silent failures on listen() setup

The component does a dynamic `import('@tauri-apps/api/event')` inside `useEffect` then calls `.listen(...)` on the module without any `.catch()`. If the import fails (e.g., running in web/test env) or if the listen setup throws, the error is silently dropped and the activity feed never populates.

**Fix:** add `.catch(console.error)` at minimum, or propagate to an error boundary.

---

### HIGH — `MeshPanel.tsx`: Dead props in component signature

`MeshPanel` declares `api: ApiConfig`, `config: AppConfig`, and `onConfigUpdate` in its `Props` interface, but the component only destructures `onClose`. All other props are received but never accessed. Callers pass live config and update callbacks expecting the panel to use them; they don't.

This is likely a refactor residue where the panel was split and the prop cleanup was missed.

**Fix:** either use the props or remove them from the interface so callers don't pass data that goes nowhere.

---

### HIGH — `BrowserPane.tsx`: Hardcoded prod port

```ts
function resolveUrl(url: string): string {
  const fileMatch = url.match(/^file:\/\/\/(.+)$/);
  if (fileMatch) {
    return `http://127.0.0.1:7685/api/serve-file?path=...`;
  }
```

Port `7685` is hardcoded as the prod port. When running in dev (`7686`) or on a non-default config, file:// URLs fail silently — the iframe just shows a 404.

**Fix:** accept `api: ApiConfig` as a prop and construct the URL from `api.host`/`api.port`, same as every other panel.

---

### MEDIUM — `useShellIntegration.ts`: Production `console.log` in hot path

Line 63:
```ts
console.log(`[spawn] Firing pending command in pane ${paneId}: ${cmd}`);
```

This fires every time shell integration dispatches a buffered command, which happens on every new terminal pane spawn. High-frequency in normal use.

---

### MEDIUM — `ChatView.tsx`: Dead state

```ts
const [isLoading, _setIsLoading] = useState(false);
```

`_setIsLoading` is never called. `isLoading` is always `false`. If loading UI was planned, the state is abandoned. If not, remove it.

---

### MEDIUM — `store/multiplexer/index.tsx`: Callbacks recreated on every state change

`closeWorkspace`, `closePane`, and `closeTab` include `state` in their `useCallback` dep arrays. Since `state` changes on every action, these functions are recreated constantly, causing any component that receives them as props to re-render unnecessarily.

**Fix:** access state via a ref (`useRef` updated in an effect), or read the specific slice needed via `useCallback(() => ... state.workspaces.find(...)`, scoping the dep to the relevant slice only.

---

### MEDIUM — `MemoryActivityTab.tsx`: Listener re-registration on scroll toggle

```ts
useEffect(() => {
  // ...setup listen("mem-activity", ...)
}, [api, autoScroll]);   // autoScroll inclusion is the problem
```

Every time the user toggles auto-scroll, the event listener is torn down and re-registered. During the gap between teardown and registration, activity events are lost.

**Fix:** use a ref for `autoScroll` instead of putting it in the dep array.

---

### MEDIUM — `usePtyConnection.ts`: Missing deps in PTY read loop `useEffect`

`onPtySpawned`, `onShellReady`, `onOutputChunk`, `jiggleResize` are called inside the PTY read loop effect but are not listed in its dep array. This is intentional (avoids reconnection loops), but means those callbacks are permanently stale after the first mount if the parent component passes new instances.

The comment in the code acknowledges this. The `stateRef` pattern used in `useAgentDetection.ts` would be cleaner — update a ref on every render, read the ref inside the effect.

---

## Architecture

### `Sidebar.tsx` (663 lines — approaching limit)

Three distinct concerns are inlined:

1. **`renderTab`** — a recursive function defined inside the component render body (lines 431–477). Should be a proper `TabTree` React component. As a plain function it can't use hooks, can't be memoized independently, and is recreated on every Sidebar render.

2. **`collectPaneIdsLocal`** (line 264) — re-implements `collectPaneIds` from `src/store/multiplexer/tree.ts`. Direct DRY violation; the tree module version already handles all cases.

3. **Inline agent spawn modal** (lines 613–661) — ~50 lines of modal UI mixed into the Sidebar return. Should be `AgentSpawnModal` component.

### `SecretsTab.tsx` (781 lines — over limit)

Clearly needs splitting. Distinct concerns that should be separate components:
- `LoginForm` / `MasterPasswordForm`
- `SecretsList` (with `SecretRow`)
- `ScopeSelector`
- `LockLogoutActions`

The file's size makes the async state bugs harder to spot (see CRITICAL items above).

### `App.tsx` (683 lines — approaching limit)

The keyboard shortcut handler is ~200 lines of inline `useEffect`. Should be extracted to `useGlobalKeybindings(config, ...)` hook.

### Inline `alert()` calls (10 locations)

`ServiceDrawer.tsx`, `CronDrawerSection.tsx`, `BroadcastToggle.tsx`, `MemoryActivityTab.tsx` all use `window.alert()` for error reporting. This blocks the UI thread and breaks the UX pattern used everywhere else in the codebase (toast notifications, `setError` state). Replace with the existing notification/toast system.

### `useGraphWorkers.ts` — listen() with try/catch fallback

The pattern of `try { await listen(...) } catch { startPolling() }` is good. Same approach should be applied to `MemoryPanel.tsx` and `MeshPanel.tsx` where the listen calls have no catch.

---

## Consistency

### Naming

- `_setIsLoading` naming (prefixed underscore = unused) is present in `ChatView.tsx` but inconsistent with how other unused destructured variables are handled. Convention should be either remove or prefix uniformly.
- `_onRename`, `_orchestrators`, `_canClose`, `_status` in `TabItem.tsx`/`WorkspaceItem.tsx` use underscore-prefix consistently for "prop received but not used" — this is fine as a local convention, though it raises the question of why those props exist in the interface at all.

### Error handling patterns

Three patterns coexist:

| Pattern | Used in |
|---------|---------|
| `setError(msg)` in component state | SecretsTab, AgentRunStream, useMeshData |
| `console.error(...)` and silent drop | NetworkTab, AgentsTab, multiplexer/index |
| `alert(msg)` blocking dialog | ServiceDrawer, CronDrawerSection, BroadcastToggle |

Should standardize on one approach. The `addNotification` / toast system from the multiplexer context is the right answer for user-visible errors.

### `interface` vs `type`

The codebase mixes `interface` (Props types: `BrowserPaneProps`, `FloatingInputProps`) and `type` aliases (`Action`, `SortKey`, `DrawerState`). This is acceptable — React component props conventionally use `interface`, discriminated unions use `type`. The pattern is internally consistent.

### TODOs

- `App.tsx` lines ~150, ~160: `// TODO: Swarm feature` — multiple inline TODOs. Not tracked in issues.
- `useShellIntegration.ts` line 76: `console.warn` for command timeout — same category as the `console.log` above.

### `key` props

Most `key={i}` usages are acceptable (static arrays, append-only streams, SVG grid lines). The problematic cases are:

- `SchemaRenderer.tsx` lines 44/83/215 — dynamic, plugin-driven, order may change (HIGH severity, documented above)
- `DropdownMenu.tsx` line 99 — menu items from `useMemo`; harmless but fragile if items become dynamic
- `AgentRunLogPopup.tsx` line 171 — event stream, append-only; harmless

### `listen()` setup pattern

Three different patterns in use:

| Pattern | Files |
|---------|-------|
| `listen(...).then(fn => { if (cancelled) fn(); else unlisten = fn; })` | `useUpdate.ts`, `useUsageSnapshot.ts`, `KeepAwakeTray.tsx` |
| `async setup() { const fn = await listen(...); if (!active) { fn(); return; } unlisteners.push(fn); }` | `useGraphWorkers.ts`, `useActivityPulses.ts` |
| `listen(...).then(fn => push).catch(() => {})` — no `cancelled` check | `store/multiplexer/index.tsx` |
| Dynamic import + no catch | `MemoryPanel.tsx`, `MeshPanel.tsx` |

The first two patterns are correct. The last two are bugs (see HIGH items). Standardize on the first pattern.

### Deprecated API usage

`navigator.platform` appears in `useXterm.ts` and `usePtyConnection.ts`. This API is deprecated and may return empty string in some Chromium versions. Should use `navigator.userAgentData.platform` with `navigator.userAgent` as fallback, or a Tauri-side OS detection command.

---

## File Size Tracking

Files at or near the 1000-line hard limit:

| File | Lines | Status |
|------|-------|--------|
| `SecretsTab.tsx` | 781 | Over 750 — split recommended |
| `Sidebar.tsx` | 663 | Approaching — extract `TabTree`, `AgentSpawnModal` |
| `App.tsx` | 683 | Approaching — extract keybindings hook |
| `store/multiplexer/index.tsx` | 543 | Fine |
| `MemoryCanvas.tsx` | ~500 | Fine (sigma/graphology integration is legitimately dense) |

---

## Files With No Issues

The following files are clean with no actionable findings:

`AgentIcon.tsx`, `streamParser.ts`, `useAgentRunStream.ts`, `FloatingInput.tsx`, `CommandPalette.tsx` (uses portal correctly), `NavigationBackBar.tsx`, `navigationStore.ts`, `HierarchyTree.tsx`, `TabItem.tsx`, `WorkspaceItem.tsx`, `StatusDot.tsx`, `Divider.tsx`, `SearchBar.tsx`, `KeepAwakeTray.tsx` (exemplary pattern), `InternalToast.tsx` (minor note only), `UpdateBadge.tsx`, `useContributions.ts`, `useGitInfo.ts`, `useTauriFileDrop.ts`, `useTerminalResize.ts`, `agentDescriptorCache.ts`, `agentProviders.ts`, `clipboardPaste.ts`, `pendingCommands.ts`, `formatters.ts`, `helpers.ts`, `types/usage.ts`, `nodeKind.ts`, `workerFocusStore.ts`, `ActivityTab.tsx`, `BroadcastToggle.tsx` (alert only), `ConnectionLine.tsx`, `CronRow.tsx`, `ServiceRow.tsx`, `ServicesTable.tsx`, `DreamCard.tsx`, `NotificationBadge.tsx`, `NotificationPanel.tsx`, `LogsFilterBar.tsx`, `LogsListRow.tsx`, `KeybindGroup.tsx`, `ProviderInstallCard.tsx`, `ProviderNotifications.tsx`, `DropdownMenu.tsx` (minor), `IntegrationsTab.tsx`, `NetworkTab.tsx` (Settings — static reference), `ShellDiagnosticsBanner.tsx`, `shared.tsx`, `useUsageSnapshot.ts`, `useUpdate.ts`, `useGraphData.ts`, `useGraphLayout.ts`, `useActivityPulses.ts`, `useGraphWorkers.ts`, `useAgentDetection.ts`, `useTabCompletion.ts`, `useTerminalInput.ts`, `SplitContainer.tsx`, `CompletionPopup.tsx`, `ContextMenu.tsx`, `parser.ts`, `MessageBubble.tsx`, `EventRenderer.tsx`.
