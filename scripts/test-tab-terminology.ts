import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

type AllowlistEntry = {
  pattern: RegExp;
  reason: string;
};

const internalLiteralAllowlist: AllowlistEntry[] = [
  { pattern: /\/api\/panes|\bpane_id\b|\bpaneId\b|\bCODETERM_PANE_ID\b|\bX-CodeTerm-Pane-Id\b/, reason: "Internal route, payload, or environment identifier retained for protocol compatibility." },
  { pattern: /--[\w-]*pane\b/, reason: "Existing CLI flag retained to avoid changing command behavior." },
  { pattern: /\bpane[-_][\w-]*/i, reason: "Internal event, storage, or fixture identifier retained for protocol compatibility." },
  { pattern: /(?:^|-)panes?(?:-|$)/i, reason: "Internal test identifier retained for selector compatibility." },
  { pattern: /\[pane:/i, reason: "Internal fallback marker retained for protocol compatibility." },
  { pattern: /\bpane\./i, reason: "Internal RPC namespace retained for protocol compatibility." },
  { pattern: /\bcodeterm pane\b/i, reason: "Deprecated command namespace is retained only for contract validation." },
  { pattern: /\{pane\}/i, reason: "Internal formatter binding retained for protocol compatibility." },
  { pattern: /^panes?$/i, reason: "Internal enum or payload value retained for protocol compatibility." },
];

const documentationInternalAllowlist: AllowlistEntry[] = [
  { pattern: /\/api\/panes|\bpane_id\b|\bpaneId\b|\bCODETERM_PANE_ID\b|\bX-CodeTerm-Pane-Id\b/, reason: "Documentation must quote the unchanged route, payload, and environment contract." },
  { pattern: /\bpane[-_][\w-]*|\bpane\.\w+|\bpanes\.(?:rs|ts|tsx|js)\b/i, reason: "Documentation must quote the unchanged internal event, storage, and source names." },
];

const internalInvariants = [
  ["pane_id", "src/lib/types.ts"],
  ["paneId", "src/lib/bridge.ts"],
  ["CODETERM_PANE_ID", "cli/src/main.rs"],
  ["/api/panes", "src/lib/bridge.ts"],
  ["assigned_pane_id", "src-tauri/crates/codeterm-types/src/plan/types.rs"],
  ['\\"kind\\":\\"Pane\\"', "src-tauri/crates/codeterm-types/src/plan/types.rs"],
  ["pane.create", "src-tauri/src/socket/server/pane.rs"],
  ["--tab <tab-or-alias>", "src-tauri/src/service/hierarchy.rs"],
  ["Mesh { peer: String, pane: String }", "src-tauri/src/service/hierarchy.rs"],
] as const;

function files(...roots: string[]): string[] {
  return execFileSync("rg", ["--files", ...roots], { encoding: "utf8" })
    .split(/\r?\n/)
    .filter(Boolean);
}

function prose(markdown: string): string {
  let fenced = false;
  return markdown
    .split(/\r?\n/)
    .map((line) => {
      if (line.trimStart().startsWith("```")) {
        fenced = !fenced;
        return "";
      }
      if (fenced || documentationInternalAllowlist.some(({ pattern }) => pattern.test(line))) return "";
      return line.replace(/`[^`]*`/g, "");
    })
    .join("\n");
}

function literals(source: string): string[] {
  return source.match(/"(?:\\.|[^"\\\r\n])*"/g) ?? [];
}

function assertNoPaneTerminology(path: string, text: string): void {
  for (const value of literals(text)) {
    const content = value.slice(1, -1);
    if (!/\bpanes?\b/i.test(content)) continue;
    if (internalLiteralAllowlist.some(({ pattern }) => pattern.test(content))) continue;
    throw new Error(`${path}: user-visible Pane terminology remains in ${value}`);
  }
}

for (const path of files("docs", ".agents")) {
  if (!path.endsWith(".md")) continue;
  const text = prose(readFileSync(path, "utf8"));
  if (/\bpanes?\b/i.test(text)) throw new Error(`${path}: user-visible Pane terminology remains in documentation prose`);
}

for (const path of files("src/components", "src/help", "cli/src")) {
  const normalizedPath = path.replace(/\\/g, "/");
  if (!/\.(ts|tsx|js|jsx|rs)$/.test(path) || /(?:\.test\.|_tests?\.|\/tests?\/)/.test(normalizedPath)) continue;
  assertNoPaneTerminology(path, readFileSync(path, "utf8"));
}

for (const [identifier, path] of internalInvariants) {
  const text = readFileSync(join(process.cwd(), path), "utf8");
  if (!text.includes(identifier)) throw new Error(`${path}: deferred internal identifier ${identifier} was renamed`);
}

console.log(`tab terminology verified; ${internalLiteralAllowlist.length} internal allowlist entries`);
