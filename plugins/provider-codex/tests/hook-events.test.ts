/**
 * Codex hook event roster + Stop hook config tests (DN-3 stage 2, S8).
 * Run: npx tsx plugins/provider-codex/tests/hook-events.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

const pluginDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

function loadCodexWithHost(host: unknown) {
  const source = fs.readFileSync(path.join(pluginDir, "plugin.js"), "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: "plugin.js" }).runInNewContext(context);
  return context.module.exports.default;
}

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => "null",
  homeDir: () => "/tmp/home",
  platform: () => "linux",
  unixNowMs: () => 1_000_000,
  path: { equal: (a: string, b: string) => a === b },
  expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
  envGet: () => null,
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readFileHead: () => null,
    readFileTail: () => null,
    readJson: () => null,
    writeJson: () => true,
    writeFile: () => true,
    removeFile: () => true,
    walkDir: () => [],
  },
  ...over,
});

console.log("\n--- declared hook event roster (manifest data) ---\n");

{
  const manifest = JSON.parse(fs.readFileSync(path.join(pluginDir, "plugin.json"), "utf8"));
  eq(
    manifest.spawn.hookEventRoster,
    [
      { event: "SessionStart", route: "session_start" },
      { event: "Stop", route: "stop" },
    ],
    "manifest declares SessionStart + Stop as roster data",
  );
  eq(
    manifest.integration.trust_grant.managedKeys,
    ["session_start", "stop"],
    "trust grant covers both managed hook keys",
  );
}

console.log("\n--- Stop hook install/uninstall ---\n");

{
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const original = '{"hooks":{"Stop":[{"hooks":[{"command":"user-stop-hook"}]}]}}';
  const files = new Map<string, string>([[hooksPath, original]]);
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (path2: string) => files.has(path2),
      readFile: (path2: string) => files.get(path2) ?? null,
      readJson: (path2: string) => {
        const value = files.get(path2);
        return value ? JSON.parse(value) : null;
      },
      writeFile: (path2: string, value: string) => { files.set(path2, value); return true; },
      writeJson: (path2: string, value: unknown) => { files.set(path2, JSON.stringify(value)); return true; },
      removeFile: (path2: string) => files.delete(path2),
    },
  }));

  const installed = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(installed.ok === true, "install succeeds", JSON.stringify(installed));
  const merged = JSON.parse(files.get(hooksPath) || "{}");
  const stopEntries = merged.hooks.Stop as Array<{ hooks?: Array<{ command?: string }> }>;
  const stopCmd = String(
    stopEntries.find((e) => e.hooks?.[0]?.command !== "user-stop-hook")?.hooks?.[0]?.command ?? "",
  );
  assert(stopCmd.indexOf("/hook/stop") !== -1, "Stop hook posts the stop route", stopCmd);
  assert(
    stopCmd.indexOf("X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}") !== -1,
    "Stop hook carries the binding-nonce header",
    stopCmd,
  );
  assert(stopCmd.indexOf("|| true") !== -1, "Stop hook post stays fire-and-forget", stopCmd);
  assert(
    stopEntries.some((e) => e.hooks?.[0]?.command === "user-stop-hook"),
    "install preserves foreign Stop hooks",
  );
  const sessionCmd = String(merged.hooks.SessionStart?.[0]?.hooks?.[0]?.command ?? "");
  assert(sessionCmd.indexOf("/hook/session_start") !== -1, "SessionStart hook still installed");

  assert(p.uninstallIntegration().ok === true, "uninstall succeeds");
  const uninstalled = JSON.parse(files.get(hooksPath) || "{}");
  assert(
    !uninstalled.hooks.Stop.some((e: any) => String(e.hooks?.[0]?.command ?? "").indexOf("/hook/stop") !== -1),
    "uninstall removes the CodeTerm Stop hook",
  );
  assert(
    uninstalled.hooks.Stop.some((e: any) => e.hooks?.[0]?.command === "user-stop-hook"),
    "uninstall preserves foreign Stop hooks",
  );
}

console.log("\n--- Windows: one script posting the event name ---\n");

{
  let writtenJson: any = null;
  const scripts = new Map<string, string>();
  const p = loadCodexWithHost(baseHost({
    platform: () => "windows",
    homeDir: () => "C:\\Users\\test",
    fs: {
      fileExists: () => false,
      readJson: () => null,
      readFile: () => null,
      writeJson: (_p: string, json: unknown) => { writtenJson = json; return true; },
      writeFile: (p2: string, body: string) => { scripts.set(p2, body); return true; },
      removeFile: () => true,
    },
  }));
  const res = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(!!res && res.ok === true, "windows install succeeds", JSON.stringify(res));
  const scriptPaths = [...scripts.keys()].filter((k) => k.endsWith(".ps1"));
  eq(scriptPaths.length, 1, "exactly one hook script is written — never one per event");
  const script = scripts.get(scriptPaths[0]) ?? "";
  assert(script.indexOf("$HookEvent") !== -1, "the script posts the event name parameter");
  assert(
    script.indexOf('param([string]$HookEvent = "session_start")') !== -1,
    "the event name defaults to session_start",
  );
  const winSession = String(writtenJson?.hooks?.SessionStart?.[0]?.hooks?.[0]?.commandWindows ?? "");
  const winStop = String(writtenJson?.hooks?.Stop?.[0]?.hooks?.[0]?.commandWindows ?? "");
  assert(winSession.indexOf("-HookEvent session_start") !== -1, "SessionStart entry passes its event name", winSession);
  assert(winStop.indexOf("-HookEvent stop") !== -1, "Stop entry passes its event name", winStop);
  assert(
    winSession.split(".ps1")[0] === winStop.split(".ps1")[0],
    "both entries invoke the same script",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
