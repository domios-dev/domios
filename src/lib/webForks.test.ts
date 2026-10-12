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
  {"file": "src/lib/transportParityScan.ts", "anchor": "if (!markerAssignment && ((ts.isPropertyAccessExpression(node) && [\"__TAURI_INTERNALS__\", \"__TAURI__\"].includes(node.name.text))", "ordinal": 1, "cls": "capability-gate", "note": "Audits direct native runtime access without choosing a frontend transport"},
  {"file": "src/lib/transportParityScan.ts", "anchor": "&& [\"__TAURI_INTERNALS__\", \"__TAURI__\"].includes(node.argumentExpression.text)))) {", "ordinal": 1, "cls": "capability-gate", "note": "Audits indexed native runtime access without choosing a frontend transport"},
  {"file": "src/store/multiplexer/index.tsx", "anchor": "dispatch({ type: \"SET_STATE\", state: guarded, pageOwnedSelection: IS_WEB });", "ordinal": 1, "cls": "platform-adaptation", "note": "Browser page owns its selection independently of desktop focus"},
  {"file": "src/store/multiplexer/index.tsx", "anchor": "if (!acceptsPageFocus(stateRef.current, tabId, IS_WEB)) return;", "ordinal": 1, "cls": "platform-adaptation", "note": "Browser page filters daemon focus against its local selection"},
  {"file": "src/store/multiplexer/index.tsx", "anchor": "if (IS_WEB) dispatch({ type: \"SET_FOCUSED_TAB\", tabId });", "ordinal": 1, "cls": "platform-adaptation", "note": "Browser focus updates the page selection locally"},

  {"file":"src/lib/nativeBridge.ts","anchor":"export function createNativeBridge(native: boolean = typeof window !== \"undefined\" && \"__TAURI_INTERNALS__\" in window): NativeBridge {","ordinal":1,"cls":"capability-gate","note":"Native capability detection is independent of the daemon HTTP transport"},
  {"file":"src/lib/daemonBootstrap.ts","anchor":"if (\"__TAURI_INTERNALS__\" in window) return undefined;","ordinal":1,"cls":"capability-gate","note":"Native capability detection is independent of the daemon HTTP transport"},
  {"file":"src/lib/daemonBootstrap.ts","anchor":"typeof window !== \"undefined\" && \"__TAURI_INTERNALS__\" in window,","ordinal":1,"cls":"capability-gate","note":"Native capability detection is independent of the daemon HTTP transport"},
  {"file":"src/lib/bridge.ts","anchor":"if (Object.prototype.hasOwnProperty.call(NATIVE_COMMANDS, cmd)) return IS_WEB","ordinal":1,"cls":"capability-gate","note":"Native capability detection is independent of the daemon HTTP transport"},
  { file: "src/components/Terminal/useAttachments.ts", anchor: "if (IS_WEB) {", ordinal: 1, cls: "capability-gate", note: "browser reads client clipboard bytes while native shell extracts OS paths" },
  { file: "src/components/Terminal/tabPanel/ChangeDetail.tsx", anchor: '<IconAction label="Open file" icon={ExternalLink} disabled={IS_WEB || !target || deleted} title={IS_WEB ? "Opening files in the OS is available only in the desktop app" : undefined} onClick={() => perform("open_folder")} />', ordinal: 1, cls: "capability-gate", note: "browser mirror cannot open a file in the host OS" },
  { file: "src/components/Terminal/tabPanel/ChangeDetail.tsx", anchor: '<IconAction label="Show in Explorer/Finder" icon={FolderOpen} disabled={IS_WEB || !target || deleted} title={IS_WEB ? "Opening files in the OS is available only in the desktop app" : undefined} onClick={() => perform("reveal_file")} />', ordinal: 1, cls: "capability-gate", note: "browser mirror cannot reveal a file in the host OS" },
  { file: "src/App.tsx", anchor: "if (!IS_WEB) {", ordinal: 1, cls: "capability-gate", note: "frontend-ready generation handshake belongs to the native shell" },
  { file: "src/components/Terminal/sessionBindingApi.ts", anchor: "if (IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "session rebind uses the web bridge mapping vs authenticated native daemon fetch" },
  { file: "src/components/Sidebar/Sidebar.tsx", anchor: "if (IS_WEB) {", ordinal: 1, cls: "platform-adaptation", note: "web folder-picker modal vs native Tauri dialog" },
  { file: "src/components/Sidebar/Sidebar.tsx", anchor: "if (IS_WEB) {", ordinal: 2, cls: "platform-adaptation", note: "web folder-picker modal vs native Tauri dialog (new workspace)" },
  { file: "src/lib/messengers/telegramBridgeStore.ts", anchor: "if (IS_WEB) return {};", ordinal: 1, cls: "transport-shim", note: "Telegram bridge API base: the browser mirror talks to its own daemon; only the desktop reads the configured API address" },
  { file: "src/components/Terminal/composerFiles.ts", anchor: "if (!IS_WEB) {", ordinal: 1, cls: "platform-adaptation", note: "browser DOM drag/drop vs native Tauri webview drag/drop" },
  { file: "src/components/Terminal/terminalOwnership.ts", anchor: "const isWeb = args.isWeb ?? IS_WEB;", ordinal: 1, cls: "capability-gate", note: "background-tab PTY suspension only applies where tabs can background (web)" },
  { file: "src/components/Terminal/voice/audio.ts", anchor: "!IS_WEB &&", ordinal: 1, cls: "platform-adaptation", note: "native macOS/iOS codec preference vs web codec order" },
  { file: "src/components/Terminal/voice/audio.ts", anchor: "if (IS_WEB) {", ordinal: 1, cls: "transport-shim", note: "persist voice blob: webUpload vs Tauri save_temp_file" },
  { file: "src/components/Terminal/voice/useVoiceRecorder.ts", anchor: "IS_WEB", ordinal: 1, cls: "platform-adaptation", note: "different mic-unavailable guidance text per host" },
  { file: "src/components/Terminal/voice/useVoiceRecorder.ts", anchor: "if (IS_WEB && \"permissions\" in navigator && typeof navigator.permissions?.query === \"function\") {", ordinal: 1, cls: "platform-adaptation", note: "Permissions API gate only reliable in browsers (native WKWebView misreports)" },
  { file: "src/components/Terminal/voice/useLocalRecordingCapture.ts", anchor: "isDesktop: !IS_WEB,", ordinal: 1, cls: "capability-gate", note: "LARK receiver capture exists only where the desktop host owns the native input device" },
  { file: "src/components/Terminal/voice/useLocalRecordingCapture.ts", anchor: "isDesktop: !IS_WEB,", ordinal: 2, cls: "capability-gate", note: "same native-device gate when the capture runtime is refreshed each render" },
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
  { file: "src/hooks/useKeyboardInset.ts", anchor: "if (!IS_WEB || typeof window === \"undefined\" || !window.visualViewport) {", ordinal: 1, cls: "platform-adaptation", note: "soft-keyboard viewport inset only applies to mobile browsers" },
  { file: "src/hooks/useTouchOptimizedUi.ts", anchor: "if (!IS_WEB || typeof window === \"undefined\") return false;", ordinal: 1, cls: "platform-adaptation", note: "touch-optimized UI only reachable via the web mirror" },
  { file: "src/hooks/useTouchOptimizedUi.ts", anchor: "IS_WEB,", ordinal: 1, cls: "platform-adaptation", note: "touch-optimized UI media query only applies to the web mirror" },
  { file: "src/lib/aiHelperSpawn.ts", anchor: "if (IS_WEB) return webFetch(AI_HELPER_SPAWN_PATH, init);", ordinal: 1, cls: "transport-shim", note: "AI helper spawn via webFetch vs native fetch" },
  { file: "src/lib/stateCache.ts", anchor: "if (!IS_WEB) return null;", ordinal: 1, cls: "capability-gate", note: "boot snapshot cache only meaningful where a network round-trip exists (web)" },
  { file: "src/lib/lifecycle.ts", anchor: "if (!IS_WEB) return () => {};", ordinal: 1, cls: "capability-gate", note: "visibility/pageshow/online wake resync is web-only (native webview inert)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "return IS_WEB && loadCachedStateSync() !== null;", ordinal: 1, cls: "capability-gate", note: "cache-hydrated-at-boot detection is web-only (native has no cache path)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "if (!IS_WEB) return empty;", ordinal: 1, cls: "capability-gate", note: "lazy-init hydration from the boot snapshot cache is web-only" },
  { file: "src/store/multiplexer/index.tsx", anchor: "if (IS_WEB) saveStateSnapshot(s);", ordinal: 1, cls: "capability-gate", note: "persist boot snapshot cache only where it can ever be read back (web)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "const hydratedAtBoot = IS_WEB && stateRef.current.generation === HYDRATED_GENERATION;", ordinal: 1, cls: "capability-gate", note: "cache-hydrated-at-boot detection is web-only (native has no cache path)" },
  { file: "src/store/multiplexer/index.tsx", anchor: "} else if (IS_WEB) {", ordinal: 1, cls: "capability-gate", note: "IndexedDB fallback hydration for the boot cache is web-only" },
  { file: "src/store/multiplexer/index.tsx", anchor: "if (IS_WEB && !wasReconciled) window.dispatchEvent(new Event(\"codeterm-cache-reconciled\"));", ordinal: 1, cls: "capability-gate", note: "cache-reconciled signal only fires where the boot cache exists (web)" },
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
