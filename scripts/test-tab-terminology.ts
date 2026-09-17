import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

type AllowlistEntry = {
  path?: RegExp;
  pattern: RegExp;
  reason: string;
};

const internalLiteralAllowlist: AllowlistEntry[] = [
  { pattern: /\/api\/tabs|\btab_id\b|\btabId\b|\bCODETERM_TAB_ID\b|\bX-CodeTerm-Tab-Id\b/, reason: "Internal route, payload, or environment identifier retained for protocol compatibility." },
  { pattern: /^(?:tab[-_][\w-]*|[\w-]*[-_]tab(?:[-_][\w-]*)?)$/i, reason: "Internal event, storage, or selector identifier retained for protocol compatibility." },
  { pattern: /^\[tab:/i, reason: "Internal fallback marker retained for protocol compatibility." },
  { pattern: /^tab\.(?:create|new|get_output|resize|kill_pty|list|inspect|focus|close|send_text|rename|describe)$/i, reason: "Internal RPC namespace retained for protocol compatibility." },
  { pattern: /^codeterm tab $/i, reason: "Manifest contract test detects the removed command namespace without exposing it to users." },
  { path: /cli\/src\/mesh_cli\/relay\.rs$/, pattern: /^tabs$/, reason: "Mesh relay route selector is an internal protocol value." },
  { path: /cli\/src\/cmd\/mesh\.rs$/, pattern: /^tabs$/, reason: "Mesh relay route selector is an internal protocol value." },
  { path: /cli\/src\/cmd\/mem\/socket\.rs$/, pattern: /^tab$/, reason: "Socket payload key is retained for protocol compatibility." },
  { path: /cli\/src\/cmd\/plan_markdown\.rs$/, pattern: /^tab$/, reason: "Plan payload key is retained for protocol compatibility." },
  { path: /cli\/src\/cmd\/plan\/task_cmd\.rs$/, pattern: /^tab$/, reason: "Plan payload key is retained for protocol compatibility." },
  { path: /cli\/src\/cmd\/plan\.rs$/, pattern: /^tab$/, reason: "Plan payload discriminator is retained for protocol compatibility." },
  { path: /src\/components\/Canvas\/types\.ts$/, pattern: /^tab$/, reason: "Canvas payload discriminator is retained for protocol compatibility." },
  { path: /src\/components\/Terminal\/TabSessionChat\.tsx$/, pattern: /^tab$/, reason: "Chat origin discriminator is retained for protocol compatibility." },
  { path: /src\/components\/Canvas\/planView\/(?:WorkingOnRowView|workingOnRow)\.tsx?$/, pattern: /^tab$/, reason: "Canvas chip discriminator is retained for protocol compatibility." },
  { path: /src\/components\/Canvas\/planView\/companionResolver\.ts$/, pattern: /^tab$/, reason: "Canvas planner discriminator is retained for protocol compatibility." },
  { path: /cli\/src\/cmd\/print\.rs$/, pattern: /^(?:tab|tabs|Tab|Tabs|TAB|TABS)$/, reason: "Serializer key mapping preserves internal payload field names before display projection." },
  { pattern: /^(?:ct-[\w-]*tab[\w-]*|--ct-tab-[\w-]+|var\(--ct-tab-[\w-]+, [^)]*\))$/, reason: "Internal CSS selectors and custom properties are not user-visible terminology." },
  { pattern: /^(?=[\w\s-]*-[\w-]*tab)[\w\s-]+$/, reason: "Hyphenated class and test-selector literals are internal identifiers, not prose." },
  { pattern: /^(?:\S+\s+)*(?:[\w-]*-tab[\w-]*|[\w-]*tab-[\w-]*)(?:\s+\S+)*$/, reason: "Class-list literals with a tab-named selector are internal styling identifiers, not prose." },
  { pattern: /^\[data-tab-id(?:=[^\]]+)?\]$/, reason: "Internal DOM data attribute retained for client-state wiring." },
];

const documentationInternalAllowlist: AllowlistEntry[] = [
  { pattern: /\/api\/tabs|\btab_id\b|\btabId\b|\bCODETERM_TAB_ID\b|\bX-CodeTerm-Tab-Id\b/, reason: "Documentation must quote the unchanged route, payload, and environment contract." },
  { pattern: /\btab[-_][\w-]*|\btab\.\w+|\btabs\.(?:rs|ts|tsx|js)\b/i, reason: "Documentation must quote the unchanged internal event, storage, and source names." },
];

const internalInvariants = [
  ["tab_id", "src/lib/types.ts"],
  ["tabId", "src/lib/bridge.ts"],
  ["CODETERM_TAB_ID", "cli/src/main.rs"],
  ["/api/tabs", "src/lib/bridge.ts"],
  ["assigned_tab_id", "src-tauri/crates/codeterm-types/src/plan/types.rs"],
  ['\\"kind\\":\\"Tab\\"', "src-tauri/crates/codeterm-types/src/plan/types.rs"],
  ["tab.create", "src-tauri/src/socket/server/tab.rs"],
  ["--tab <tab-or-alias>", "src-tauri/src/service/hierarchy.rs"],
  ["Mesh { peer: String, tab: String }", "src-tauri/src/service/hierarchy.rs"],
] as const;

const userVisibleCommandInvariants = [
  ["codeterm send --tab X", "cli/src/args/cli.rs"],
  ["--tab", "cli/src/args/orchestrator.rs"],
  ["codeterm orchestrator status --plan {plan_id} --tab {tab}", "src-tauri/crates/codeterm-plan/src/service_plan/heartbeat.rs"],
  ["--tab {tab_id}", "src-tauri/src/service/spawn_factory.rs"],
  ["codeterm send --tab", "packages/chat-ui/src/codeterm/templates.test.ts"],
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

function assertNoTabTerminology(path: string, text: string): void {
  for (const value of literals(text)) {
    const content = value.slice(1, -1);
    const proseContent = content.replace(/\{tab\}/g, "");
    if (!/\btabs?\b/i.test(proseContent)) continue;
    if (internalLiteralAllowlist.some(({ path: allowedPath, pattern }) => (!allowedPath || allowedPath.test(path.replace(/\\/g, "/"))) && pattern.test(content))) continue;
    throw new Error(`${path}: user-visible Tab terminology remains in ${value}`);
  }
}

function assertNoDeprecatedTabFlag(path: string, text: string): void {
  if (/--[\w-]*tab\b/.test(text)) throw new Error(`${path}: tab-named CLI flags are not supported user-visible flags; use --tab`);
}

for (const path of files("essences", "prod-docs", ".agents")) {
  if (!path.endsWith(".md")) continue;
  const text = prose(readFileSync(path, "utf8"));
  if (/\btabs?\b/i.test(text)) throw new Error(`${path}: user-visible Tab terminology remains in documentation prose`);
}

for (const path of files("src/components", "src/help", "cli/src")) {
  const normalizedPath = path.replace(/\\/g, "/");
  if (!/\.(ts|tsx|js|jsx|rs)$/.test(path) || /(?:\.test\.|_tests?\.|\/tests?\/|(?:^|\/)tests?\.rs$|fixture|testdata)/i.test(normalizedPath)) continue;
  assertNoTabTerminology(path, readFileSync(path, "utf8"));
}

for (const path of files("cli", "src-tauri", "packages", "essences", "prod-docs", ".agents")) {
  if (path.endsWith(".css")) continue;
  assertNoDeprecatedTabFlag(path, readFileSync(path, "utf8"));
}

for (const [identifier, path] of internalInvariants) {
  const text = readFileSync(join(process.cwd(), path), "utf8");
  if (!text.includes(identifier)) throw new Error(`${path}: deferred internal identifier ${identifier} was renamed`);
}

for (const [command, path] of userVisibleCommandInvariants) {
  const text = readFileSync(join(process.cwd(), path), "utf8");
  if (!text.includes(command)) throw new Error(`${path}: canonical command ${command} is missing`);
}

console.log(`tab terminology verified; ${internalLiteralAllowlist.length} internal allowlist entries`);
