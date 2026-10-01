/**
 * Fork inventory (spec S1 "Parity principle & code reuse"): every `IS_WEB` /
 * `isTauriEnv` / `__TAURI_INTERNALS__` branch site in `src/` must be listed here
 * with a decision-vocabulary class. An unlisted (new or moved) fork fails the
 * scan below, forcing the classification conversation at PR time instead of
 * letting a class-(c) parity defect slip in silently.
 *
 * Classes (spec S1 decision vocabulary):
 *   platform-adaptation — UI genuinely different per host (allowed)
 *   transport-shim       — same behavior, different wire (bridge/webApi/webEvents)
 *   capability-gate      — feature present iff daemon/browser capability present
 * "parity-defect" (feature silently absent on web) has zero entries by design —
 * that classification exists in the vocabulary but must never appear below.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

type ForkClass = "platform-adaptation" | "transport-shim" | "capability-gate";

interface ForkSite {
  file: string;
  anchor: string;
  ordinal: number;
  cls: ForkClass;
  note: string;
}

const INVENTORY: ForkSite[] = [
  { file: "src/App.tsx", anchor: "if (!IS_WEB) {", ordinal: 1, cls: "capability-gate", note: "frontend-ready generation handshake belongs to the native shell" },
  { file: "src/components/Terminal/NewSessionPicker.tsx", anchor: "if (IS_WEB) {", ordinal: 1, cls: "platform-adaptation", note: "new-workspace: native folder dialog on desktop vs name-input fallback on web" },
  { file: "src/components/Terminal/sessionBindingApi.ts", anchor: "if (IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "session rebind uses the web bridge mapping vs authenticated native daemon fetch" },
  { file: "src/components/Sidebar/Sidebar.tsx", anchor: "if (IS_WEB) {", ordinal: 1, cls: "platform-adaptation", note: "web folder-picker modal vs native Tauri dialog" },
  { file: "src/components/Sidebar/Sidebar.tsx", anchor: "if (IS_WEB) {", ordinal: 2, cls: "platform-adaptation", note: "web folder-picker modal vs native Tauri dialog (new workspace)" },
  { file: "src/components/StaleBuildBanner.tsx", anchor: "const apiBase = IS_WEB", ordinal: 1, cls: "transport-shim", note: "build-check API base: window.location.origin vs local dev daemon" },
  { file: "src/components/FrontendCrashBoundary.tsx", anchor: "const appWindow = \"__TAURI_INTERNALS__\" in window ? getCurrentWindow() : null;", ordinal: 1, cls: "platform-adaptation", note: "native crash fallback can reload the Tauri window directly" },
  { file: "src/components/Terminal/composerFiles.ts", anchor: "if (!IS_WEB) {", ordinal: 1, cls: "platform-adaptation", note: "browser DOM drag/drop vs native Tauri webview drag/drop" },
  { file: "src/components/Terminal/terminalOwnership.ts", anchor: "const isWeb = args.isWeb ?? IS_WEB;", ordinal: 1, cls: "capability-gate", note: "background-tab PTY suspension only applies where tabs can background (web)" },
  { file: "src/components/Terminal/voice/audio.ts", anchor: "!IS_WEB &&", ordinal: 1, cls: "platform-adaptation", note: "native macOS/iOS codec preference vs web codec order" },
  { file: "src/components/Terminal/voice/audio.ts", anchor: "if (IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "persist voice blob: webUpload vs Tauri save_temp_file" },
  { file: "src/components/Terminal/voice/useVoiceRecorder.ts", anchor: "IS_WEB", ordinal: 1, cls: "platform-adaptation", note: "different mic-unavailable guidance text per host" },
  { file: "src/components/Terminal/voice/useVoiceRecorder.ts", anchor: "if (IS_WEB && \"permissions\" in navigator && typeof navigator.permissions?.query === \"function\") {", ordinal: 1, cls: "platform-adaptation", note: "Permissions API gate only reliable in browsers (native WKWebView misreports)" },
  { file: "src/components/Titlebar/Titlebar.tsx", anchor: "const isTauri = !IS_WEB;", ordinal: 1, cls: "platform-adaptation", note: "native window-chrome controls (traffic lights / min/max/close)" },
  { file: "src/components/Titlebar/Titlebar.tsx", anchor: "className={`flex min-w-0 items-center bg-[var(--ct-titlebar-bg)] select-none shrink-0 ${IS_WEB ? \"relative z-50 max-sm:min-h-[calc(44px+var(--ct-safe-top,0px))]\" : \"\"}`}", ordinal: 1, cls: "platform-adaptation", note: "mobile web titlebar safe-area min-height and stacking above the drawer; native chrome has fixed height" },
  { file: "src/components/Titlebar/Titlebar.tsx", anchor: "{IS_WEB && (", ordinal: 1, cls: "platform-adaptation", note: "web-only peer picker lives in the titlebar so host switching never depends on peer state" },
  { file: "src/components/WebGate/PwaSetup.tsx", anchor: "if (!IS_WEB) return;", ordinal: 1, cls: "platform-adaptation", note: "service worker registration is web-only" },
  { file: "src/components/WebGate/PwaSetup.tsx", anchor: "if (!IS_WEB || isStandalone()) return;", ordinal: 1, cls: "platform-adaptation", note: "PWA install-prompt banner is web-only" },
  { file: "src/lib/serviceWorkerRegistration.ts", anchor: "if (!IS_WEB || !(\"serviceWorker\" in navigator)) return;", ordinal: 1, cls: "platform-adaptation", note: "service worker registration guard is web-only" },
  { file: "src/lib/hostBridge.ts", anchor: "if (!IS_WEB) return true;", ordinal: 1, cls: "capability-gate", note: "host bridge calls (plugin tab menu, Telegram thread state) need a host: always in Tauri, only with an authenticated web session" },
  { file: "src/components/Terminal/composerAttachmentHandoff.ts", anchor: "relayPeerId: () => (IS_WEB ? relayTarget()?.id ?? null : null),", ordinal: 1, cls: "platform-adaptation", note: "only the web mirror has a relay peer view; the desktop app addresses remote tabs by composite mesh ids" },
  { file: "src/components/WebGate/WebGate.tsx", anchor: "const [state, setState] = useState<GateState>(IS_WEB ? \"checking\" : \"ready\");", ordinal: 1, cls: "platform-adaptation", note: "token auth gate only exists for the browser entry point" },
  { file: "src/components/WebGate/WebGate.tsx", anchor: "if (!IS_WEB) return;", ordinal: 1, cls: "capability-gate", note: "browser-gateway availability events only exist where a gateway sits between the app and the daemon (web)" },
  { file: "src/components/WebGate/WebGate.tsx", anchor: "if (!IS_WEB) return;", ordinal: 2, cls: "platform-adaptation", note: "URL-fragment token consumption only applies to a browser tab" },
  { file: "src/components/WebGate/WebGate.tsx", anchor: "{IS_WEB && <PwaSetup />}", ordinal: 1, cls: "platform-adaptation", note: "PwaSetup only mounted on web" },
  { file: "src/components/Terminal/ChatComposer.composition.harness.ts", anchor: "(win as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};", ordinal: 1, cls: "platform-adaptation", note: "test harness plants the Tauri runtime marker so bridge resolves IS_WEB=false and unmocked invokes fail loudly" },
  { file: "src/hooks/terminal/usePtyConnection.ts", anchor: "const historySource = terminalHistorySource(ptyId, IS_WEB, isRemote);", ordinal: 1, cls: "transport-shim", note: "seeded backend history is replayed natively, while web and remote terminals use their cursor-aware output streams" },
  { file: "src/hooks/terminal/usePtyConnection.ts", anchor: "if (ptyId && IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "web terminal output uses its stream cursor or resets the long-poll tail when streaming is disabled" },
  { file: "src/hooks/terminal/usePtyConnection.ts", anchor: "if (IS_WEB && !useOutputStream) {", ordinal: 1, cls: "capability-gate", note: "boot-changed offset revalidation only matters where the read cursor is a resumable HTTP byte offset (web); the multiplexed output stream self-heals boot changes instead" },
  { file: "src/hooks/terminal/useTerminalResize.ts", anchor: "const nativeResizeCommit = !IS_WEB", ordinal: 1, cls: "platform-adaptation", note: "Windows native resize commits on the host window's resize boundary; web commits continuously" },
  { file: "src/hooks/terminal/useTauriFileDrop.ts", anchor: "if (!(\"__TAURI_INTERNALS__\" in window)) return;", ordinal: 1, cls: "platform-adaptation", note: "native Tauri webview drag/drop is a no-op on web (see composerFiles.ts web path)" },
  { file: "src/hooks/useKeyboardInset.ts", anchor: "if (!IS_WEB || typeof window === \"undefined\" || !window.visualViewport) {", ordinal: 1, cls: "platform-adaptation", note: "soft-keyboard viewport inset only applies to mobile browsers" },
  { file: "src/hooks/useTouchOptimizedUi.ts", anchor: "if (!IS_WEB || typeof window === \"undefined\") return false;", ordinal: 1, cls: "platform-adaptation", note: "touch-optimized UI only reachable via the web mirror" },
  { file: "src/hooks/useTouchOptimizedUi.ts", anchor: "IS_WEB,", ordinal: 1, cls: "platform-adaptation", note: "touch-optimized UI media query only applies to the web mirror" },
  { file: "src/lib/bridge.ts", anchor: "if (IS_WEB) return err instanceof WebApiError && err.status === 404;", ordinal: 1, cls: "transport-shim", note: "dead-PTY classification: WebApiError 404 vs native \"PTY not found\"/\"PTY closed\" message match" },
  { file: "src/lib/bridge.ts", anchor: "if (!IS_WEB) return { supported: true };", ordinal: 1, cls: "capability-gate", note: "webCapability() — query whether an IPC cmd is reachable on web" },
  { file: "src/lib/bridge.ts", anchor: "if (!IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "THE seam: invoke() routes to tauriInvoke vs HTTP_MAP" },
  { file: "src/lib/bridge.ts", anchor: "if (IS_WEB && event === \"config-changed\" && e.payload && typeof e.payload === \"object\") {", ordinal: 1, cls: "transport-shim", note: "config-changed payload carries the browser's own token; native reads it from the daemon config" },
  { file: "src/lib/bridge.ts", anchor: "if (localListenBranch(IS_WEB) === \"tauri\") {", ordinal: 1, cls: "transport-shim", note: "THE seam: listen() routes to tauriListen vs webListen" },
  { file: "src/lib/aiHelperSpawn.ts", anchor: "if (IS_WEB) return webFetch(AI_HELPER_SPAWN_PATH, init);", ordinal: 1, cls: "transport-shim", note: "AI helper spawn via webFetch vs native fetch" },
  { file: "src/lib/resizeOwner.ts", anchor: "const prefix = IS_WEB ? \"web:\" : \"tauri:\";", ordinal: 1, cls: "capability-gate", note: "resize-owner client-id namespacing to avoid native/web arbitration collisions" },
  { file: "src/lib/stateCache.ts", anchor: "if (!IS_WEB) return null;", ordinal: 1, cls: "capability-gate", note: "boot snapshot cache only meaningful where a network round-trip exists (web)" },
  { file: "src/lib/webApi.ts", anchor: "if (IS_WEB) return apiHost();", ordinal: 1, cls: "transport-shim", note: "THE seam: daemonHttpBase() — canonical daemon origin per access mode" },
  { file: "src/lib/webApi.ts", anchor: "return (IS_WEB ? currentToken() : api?.token) ?? \"\";", ordinal: 1, cls: "transport-shim", note: "THE seam: daemonAuthToken() — canonical auth token per access mode" },
  { file: "src/lib/webApi.ts", anchor: "if (IS_WEB) return webFetch(path, init);", ordinal: 1, cls: "transport-shim", note: "daemon REST fetch routes selected web peers through the authenticated relay and native callers directly" },
  { file: "src/components/Canvas/usePlanPeerContext.ts", anchor: "if (!IS_WEB) return;", ordinal: 1, cls: "transport-shim", note: "web peer reset cancels and clears plan queries before the selected machine rebinds" },
  { file: "src/lib/lifecycle.ts", anchor: "if (!IS_WEB) return () => {};", ordinal: 1, cls: "capability-gate", note: "visibility/pageshow/online wake resync is web-only (native webview inert)" },
  { file: "src/lib/webEvents.ts", anchor: "if (started || !IS_WEB) return;", ordinal: 1, cls: "transport-shim", note: "THE seam: web-only SSE bootstrap guard (no-op on native); relay target byte-piped via relay-stream" },
  { file: "src/lib/outputStream.ts", anchor: "return IS_WEB && outputStreamEnabled();", ordinal: 1, cls: "transport-shim", note: "multiplexed PTY output over HTTP (web); relay target byte-piped via relay-stream" },
  { file: "src/lib/outputStream.ts", anchor: "const target = IS_WEB ? relayTarget() : null;", ordinal: 1, cls: "transport-shim", note: "whole-peer web subscriptions qualify plain tab ids for the existing scoped relay transport" },
  { file: "src/store/multiplexer/index.tsx", anchor: "return IS_WEB && loadCachedStateSync() !== null;", ordinal: 1, cls: "capability-gate", note: "cache-hydrated-at-boot detection is web-only (native has no cache path)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "if (!IS_WEB) return empty;", ordinal: 1, cls: "capability-gate", note: "lazy-init hydration from the boot snapshot cache is web-only" },
  { file: "src/store/multiplexer/index.tsx", anchor: "if (IS_WEB) saveStateSnapshot(s);", ordinal: 1, cls: "capability-gate", note: "persist boot snapshot cache only where it can ever be read back (web)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "const hydratedAtBoot = IS_WEB && stateRef.current.generation === HYDRATED_GENERATION;", ordinal: 1, cls: "capability-gate", note: "cache-hydrated-at-boot detection is web-only (native has no cache path)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "} else if (IS_WEB) {", ordinal: 1, cls: "capability-gate", note: "IndexedDB fallback hydration for the boot cache is web-only" },
  { file: "src/store/multiplexer/index.tsx", anchor: "if (IS_WEB && !wasReconciled) window.dispatchEvent(new Event(\"codeterm-cache-reconciled\"));", ordinal: 1, cls: "capability-gate", note: "cache-reconciled signal only fires where the boot cache exists (web)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "const useRestSpawn = IS_WEB;", ordinal: 1, cls: "transport-shim", note: "spawn agent tab via webJson vs Tauri invoke" },
  { file: "src/store/multiplexer/index.tsx", anchor: "const result = IS_WEB", ordinal: 1, cls: "transport-shim", note: "create orchestration pair via webJson vs Tauri invoke" },
  { file: "src/components/Canvas/plansApi.ts", anchor: "if (!IS_WEB) return invokePlanRead(\"get_plan_full\", { planId }, signal);", ordinal: 1, cls: "transport-shim", note: "single plan read: desktop goes straight to PlanStore over IPC, web keeps REST" },
  { file: "src/components/Canvas/plansApi.ts", anchor: "if (!IS_WEB) return invokePlanRead(\"list_plan_index\", undefined, signal);", ordinal: 1, cls: "transport-shim", note: "plan index read: desktop goes straight to PlanStore over IPC, web keeps REST" },
  { file: "src/components/Canvas/plansApi.ts", anchor: "if (!IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "batched full-plan read: desktop goes straight to PlanStore over IPC, web keeps REST" },
];

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const SRC = path.join(ROOT, "src");
const FORK_RE = /\bIS_WEB\b|\bisTauriEnv\b|__TAURI_INTERNALS__/;

function walk(dir: string, out: string[]): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === "dist") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, out);
    } else if (/\.(ts|tsx)$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

function redact(content: string): string {
  const blank = (m: string) => m.replace(/[^\n]/g, " ");
  let redacted = content.replace(/import\b[^;]*;/g, blank);
  redacted = redacted.replace(/export\s*\{[^}]*\}\s*;/g, blank);
  redacted = redacted.replace(/export const IS_WEB[^;]*;/g, blank);
  return redacted;
}

function forkKey(file: string, anchor: string, ordinal: number): string {
  return `${file}:${anchor.trim().replace(/\s+/g, " ")}:${ordinal}`;
}

function scanForkSites(): Set<string> {
  const sites = new Set<string>();
  for (const file of walk(SRC, [])) {
    const content = fs.readFileSync(file, "utf8");
    const lines = redact(content).split("\n");
    const anchors = new Map<string, number>();
    lines.forEach((line) => {
      const trimmed = line.trim();
      if (!trimmed) return;
      if (trimmed.startsWith("//") || trimmed.startsWith("/*") || trimmed.startsWith("*")) return;
      if (FORK_RE.test(line)) {
        const anchor = line.trim().replace(/\s+/g, " ");
        const ordinal = (anchors.get(anchor) ?? 0) + 1;
        anchors.set(anchor, ordinal);
        sites.add(forkKey(path.relative(ROOT, file).split(path.sep).join("/"), anchor, ordinal));
      }
    });
  }
  return sites;
}

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("webForks fork inventory:");

const scanned = scanForkSites();
const declared = new Set(INVENTORY.map((s) => forkKey(s.file, s.anchor, s.ordinal)));

assert(`inventory has ${INVENTORY.length} entries with unique fork anchors`, declared.size === INVENTORY.length);

const unlisted = [...scanned].filter((k) => !declared.has(k)).sort();
assert(
  unlisted.length === 0
    ? "no unlisted fork sites found in src/"
    : `unlisted fork site(s) found (must be added to INVENTORY with a class): ${unlisted.join(", ")}`,
  unlisted.length === 0,
);

const stale = [...declared].filter((k) => !scanned.has(k)).sort();
assert(
  stale.length === 0
    ? "no stale inventory entries (every declared site still exists in src/)"
    : `stale inventory entries (site moved/removed — update INVENTORY): ${stale.join(", ")}`,
  stale.length === 0,
);

assert(
  "diagnostics_that_speak insertion above an inventoried fork preserves its anchor",
  forkKey("src/fixture.ts", "  if (IS_WEB) {", 1) === forkKey("src/fixture.ts", "if (IS_WEB) {", 1),
);
assert(
  "diagnostics_that_speak a duplicate-guard fork remains unlisted",
  !new Set([forkKey("src/fixture.ts", "if (IS_WEB) {", 1)]).has(forkKey("src/fixture.ts", "if (IS_WEB) {", 2)),
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
