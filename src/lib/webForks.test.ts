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
  line: number;
  cls: ForkClass;
  note: string;
}

const INVENTORY: ForkSite[] = [
  { file: "src/App.tsx", line: 68, cls: "capability-gate", note: "frontend-ready generation handshake belongs to the native shell" },
  { file: "src/components/MeshPanel/MeshPanel.tsx", line: 208, cls: "transport-shim", note: "spawn setup chat via webJson vs local fetch" },
  { file: "src/components/Terminal/NewSessionPicker.tsx", line: 150, cls: "platform-adaptation", note: "new-workspace: native folder dialog on desktop vs name-input fallback on web" },
  { file: "src/components/Sidebar/Sidebar.tsx", line: 157, cls: "platform-adaptation", note: "web folder-picker modal vs native Tauri dialog" },
  { file: "src/components/Sidebar/Sidebar.tsx", line: 246, cls: "platform-adaptation", note: "web folder-picker modal vs native Tauri dialog (new workspace)" },
  { file: "src/components/Sidebar/Sidebar.tsx", line: 562, cls: "platform-adaptation", note: "web-only peer picker lives in the General sidebar header" },
  { file: "src/components/StaleBuildBanner.tsx", line: 18, cls: "transport-shim", note: "build-check API base: window.location.origin vs local dev daemon" },
  { file: "src/components/FrontendCrashBoundary.tsx", line: 8, cls: "platform-adaptation", note: "native crash fallback can reload the Tauri window directly" },
  { file: "src/components/Terminal/composerFiles.ts", line: 27, cls: "platform-adaptation", note: "browser DOM drag/drop vs native Tauri webview drag/drop" },
  { file: "src/components/Terminal/terminalOwnership.ts", line: 9, cls: "capability-gate", note: "background-tab PTY suspension only applies where tabs can background (web)" },
  { file: "src/components/Terminal/voice/audio.ts", line: 180, cls: "platform-adaptation", note: "native macOS/iOS codec preference vs web codec order" },
  { file: "src/components/Terminal/voice/audio.ts", line: 222, cls: "transport-shim", note: "persist voice blob: webUpload vs Tauri save_temp_file" },
  { file: "src/components/Terminal/voice/useVoiceRecorder.ts", line: 271, cls: "platform-adaptation", note: "different mic-unavailable guidance text per host" },
  { file: "src/components/Terminal/voice/useVoiceRecorder.ts", line: 278, cls: "platform-adaptation", note: "Permissions API gate only reliable in browsers (native WKWebView misreports)" },
  { file: "src/components/Titlebar/Titlebar.tsx", line: 55, cls: "platform-adaptation", note: "native window-chrome controls (traffic lights / min/max/close)" },
  { file: "src/components/Titlebar/Titlebar.tsx", line: 65, cls: "platform-adaptation", note: "mobile web titlebar safe-area min-height class (notch/safe-top); native chrome has fixed height" },
  { file: "src/components/WebGate/PwaSetup.tsx", line: 67, cls: "platform-adaptation", note: "service worker registration is web-only" },
  { file: "src/components/WebGate/PwaSetup.tsx", line: 87, cls: "platform-adaptation", note: "PWA install-prompt banner is web-only" },
  { file: "src/lib/serviceWorkerRegistration.ts", line: 52, cls: "platform-adaptation", note: "service worker registration guard is web-only" },
  { file: "src/components/WebGate/WebGate.tsx", line: 30, cls: "platform-adaptation", note: "token auth gate only exists for the browser entry point" },
  { file: "src/components/WebGate/WebGate.tsx", line: 37, cls: "capability-gate", note: "browser-gateway availability events only exist where a gateway sits between the app and the daemon (web)" },
  { file: "src/components/WebGate/WebGate.tsx", line: 67, cls: "platform-adaptation", note: "URL-fragment token consumption only applies to a browser tab" },
  { file: "src/components/WebGate/WebGate.tsx", line: 151, cls: "platform-adaptation", note: "PwaSetup only mounted on web" },
  { file: "src/hooks/terminal/usePtyConnection.ts", line: 218, cls: "transport-shim", note: "scrollback replay: web tail-reset vs native history invoke" },
  { file: "src/hooks/terminal/usePtyConnection.ts", line: 225, cls: "transport-shim", note: "initial-history admission takes the host into account: only the native transport fetches tab history up front" },
  { file: "src/hooks/terminal/usePtyConnection.ts", line: 391, cls: "capability-gate", note: "boot-changed offset revalidation only matters where the read cursor is a resumable HTTP byte offset (web); the multiplexed output stream self-heals boot changes instead" },
  { file: "src/hooks/terminal/useTerminalResize.ts", line: 29, cls: "platform-adaptation", note: "Windows native resize commits on the host window's resize boundary; web commits continuously" },
  { file: "src/hooks/terminal/useTauriFileDrop.ts", line: 33, cls: "platform-adaptation", note: "native Tauri webview drag/drop is a no-op on web (see composerFiles.ts web path)" },
  { file: "src/hooks/useKeyboardInset.ts", line: 26, cls: "platform-adaptation", note: "soft-keyboard viewport inset only applies to mobile browsers" },
  { file: "src/hooks/useTouchOptimizedUi.ts", line: 10, cls: "platform-adaptation", note: "touch-optimized UI only reachable via the web mirror" },
  { file: "src/hooks/useTouchOptimizedUi.ts", line: 12, cls: "platform-adaptation", note: "touch-optimized UI media query only applies to the web mirror" },
  { file: "src/lib/bridge.ts", line: 80, cls: "transport-shim", note: "dead-PTY classification: WebApiError 404 vs native \"PTY not found\"/\"PTY closed\" message match" },
  { file: "src/lib/bridge.ts", line: 662, cls: "capability-gate", note: "webCapability() — query whether an IPC cmd is reachable on web" },
  { file: "src/lib/bridge.ts", line: 675, cls: "transport-shim", note: "THE seam: invoke() routes to tauriInvoke vs HTTP_MAP" },
  { file: "src/lib/bridge.ts", line: 830, cls: "transport-shim", note: "config-changed payload carries the browser's own token; native reads it from the daemon config" },
  { file: "src/lib/bridge.ts", line: 840, cls: "transport-shim", note: "THE seam: listen() routes to tauriListen vs webListen" },
  { file: "src/lib/pluginConfigure.ts", line: 44, cls: "transport-shim", note: "spawn configure-agent via webJson vs local fetch" },
  { file: "src/lib/resizeOwner.ts", line: 17, cls: "capability-gate", note: "resize-owner client-id namespacing to avoid native/web arbitration collisions" },
  { file: "src/lib/stateCache.ts", line: 43, cls: "capability-gate", note: "boot snapshot cache only meaningful where a network round-trip exists (web)" },
  { file: "src/lib/webApi.ts", line: 138, cls: "transport-shim", note: "THE seam: daemonHttpBase() — canonical daemon origin per access mode" },
  { file: "src/lib/webApi.ts", line: 145, cls: "transport-shim", note: "THE seam: daemonAuthToken() — canonical auth token per access mode" },
  { file: "src/lib/lifecycle.ts", line: 56, cls: "capability-gate", note: "visibility/pageshow/online wake resync is web-only (native webview inert)" },
  { file: "src/lib/webEvents.ts", line: 180, cls: "transport-shim", note: "THE seam: web-only SSE bootstrap guard (no-op on native); relay target byte-piped via relay-stream" },
  { file: "src/lib/outputStream.ts", line: 115, cls: "transport-shim", note: "multiplexed PTY output over HTTP (web); relay target byte-piped via relay-stream" },
  { file: "src/store/multiplexer/index.tsx", line: 61, cls: "capability-gate", note: "cache-hydrated-at-boot detection is web-only (native has no cache path)" },
  { file: "src/store/multiplexer/index.tsx", line: 78, cls: "capability-gate", note: "lazy-init hydration from the boot snapshot cache is web-only" },
  { file: "src/store/multiplexer/index.tsx", line: 143, cls: "capability-gate", note: "persist boot snapshot cache only where it can ever be read back (web)" },
  { file: "src/store/multiplexer/index.tsx", line: 169, cls: "capability-gate", note: "cache-hydrated-at-boot detection is web-only (native has no cache path)" },
  { file: "src/store/multiplexer/index.tsx", line: 173, cls: "capability-gate", note: "IndexedDB fallback hydration for the boot cache is web-only" },
  { file: "src/store/multiplexer/index.tsx", line: 187, cls: "capability-gate", note: "cache-reconciled signal only fires where the boot cache exists (web)" },
  { file: "src/store/multiplexer/index.tsx", line: 601, cls: "transport-shim", note: "spawn agent tab via webJson vs Tauri invoke" },
  { file: "src/store/multiplexer/index.tsx", line: 661, cls: "transport-shim", note: "create orchestration pair via webJson vs Tauri invoke" },
  { file: "src/components/Canvas/plansApi.ts", line: 79, cls: "transport-shim", note: "single plan read: desktop goes straight to PlanStore over IPC, web keeps REST" },
  { file: "src/components/Canvas/plansApi.ts", line: 111, cls: "transport-shim", note: "plan index read: desktop goes straight to PlanStore over IPC, web keeps REST" },
  { file: "src/components/Canvas/plansApi.ts", line: 143, cls: "transport-shim", note: "batched full-plan read: desktop goes straight to PlanStore over IPC, web keeps REST" },
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

function scanForkSites(): Set<string> {
  const sites = new Set<string>();
  for (const file of walk(SRC, [])) {
    const content = fs.readFileSync(file, "utf8");
    const lines = redact(content).split("\n");
    lines.forEach((line, i) => {
      const trimmed = line.trim();
      if (!trimmed) return;
      if (trimmed.startsWith("//") || trimmed.startsWith("/*") || trimmed.startsWith("*")) return;
      if (FORK_RE.test(line)) {
        sites.add(`${path.relative(ROOT, file).split(path.sep).join("/")}:${i + 1}`);
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
const declared = new Set(INVENTORY.map((s) => `${s.file}:${s.line}`));

assert(`inventory has ${INVENTORY.length} entries with unique file:line keys`, declared.size === INVENTORY.length);

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

for (const site of INVENTORY) {
  const full = path.join(ROOT, site.file);
  const lines = fs.readFileSync(full, "utf8").split("\n");
  const text = lines[site.line - 1] ?? "";
  assert(
    `${site.file}:${site.line} still references a fork identifier ("${text.trim()}")`,
    FORK_RE.test(text),
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
