/**
 * Codex pane-state classifier tests (AS2 — Case A: self-update prompt auto-skip).
 * Run: npx tsx plugins/provider-codex/tests/classify-pane-state.test.ts
 *
 * Loads the built QuickJS bundle (plugin.js) and exercises classifyPaneState
 * against golden fixtures: the codex update prompt must classify as
 * `update_prompt` with a Skip-not-Update key action above the reactor's
 * confidence threshold; an auth/login screen must classify as `waiting_on_auth`
 * with NO auto-press; a negative corpus of normal output must never fire.
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

// Mirror the core reactor gate (src-tauri/src/agent/state_reactor.rs).
const GLOBAL_THRESHOLD = 0.8;

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

const here = path.dirname(fileURLToPath(import.meta.url));

function loadCodex() {
  const pluginPath = path.join(here, "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const host = {
    manifestJson: () => "null",
    homeDir: () => "/tmp/home",
    platform: () => "linux",
    envGet: () => null,
    unixNowMs: () => 0,
    log: () => {},
    shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
    fs: { fileExists: () => false, readFile: () => null, readJson: () => null, writeJson: () => true, writeFile: () => true },
  };
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

function fixture(name: string): string {
  return fs.readFileSync(path.join(here, "fixtures", "pane-state", name), "utf8");
}

function classify(plugin: any, screenText: string) {
  return plugin.classifyPaneState({
    paneId: "pane-1",
    agentType: "codex",
    screenText,
  });
}

const plugin = loadCodex();

assert(typeof plugin.classifyPaneState === "function", "classifyPaneState is exported");

console.log("\n--- positive: update prompt, Skip is option 2 ---\n");
{
  const r = classify(plugin, fixture("update-prompt-skip-option2.txt"));
  eq(r?.state, "update_prompt", "state is update_prompt");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  eq(r?.action?.keys, ["down", "enter"], "navigates one down to Skip, then enter");
  // The cardinal sin guard: a blind enter (or no down) would accept the default
  // highlight "Update now". The action must move OFF option 1 before enter.
  assert(
    Array.isArray(r?.action?.keys) && r.action.keys.indexOf("down") !== -1,
    "action moves off the default 'Update now' highlight",
  );
  assert(
    Array.isArray(r?.action?.keys) && r.action.keys[r.action.keys.length - 1] === "enter",
    "action ends with enter",
  );
}

console.log("\n--- positive: update prompt, Skip is option 3 (dynamic navigation) ---\n");
{
  const r = classify(plugin, fixture("update-prompt-skip-option3.txt"));
  eq(r?.state, "update_prompt", "state is update_prompt");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  eq(r?.action?.keys, ["down", "down", "enter"], "navigates two downs to Skip (option 3), then enter");
}

console.log("\n--- auth guard: login screen → waiting_on_auth, NEVER auto-acted ---\n");
{
  const r = classify(plugin, fixture("auth-login.txt"));
  eq(r?.state, "waiting_on_auth", "auth/login classified as waiting_on_auth");
  assert(!r?.action?.keys, "auth prompt carries NO key action (never auto-pressed)", JSON.stringify(r?.action));
}

console.log("\n--- negatives: normal output never fires (no false positives) ---\n");
for (const f of ["normal-working.txt", "tool-call-list.txt", "diff-and-shell.txt"]) {
  const r = classify(plugin, fixture(f));
  const state = r?.state ?? "unknown";
  assert(state !== "update_prompt", `${f}: not classified update_prompt`, `state=${state}`);
  assert(state !== "waiting_on_auth", `${f}: not classified waiting_on_auth`, `state=${state}`);
  assert(!r?.action?.keys, `${f}: no auto-key action`, JSON.stringify(r?.action));
}

console.log("\n--- ensemble: a single weak signal must NOT reach threshold ---\n");
{
  // "Update available" + a version arrow, but NO update/skip menu → below
  // threshold (this is the diff-and-shell npm-notice shape, isolated).
  const weak = "Some log line: New version available! 1.0.0 -> 1.1.0\nnothing actionable here\n";
  const r = classify(plugin, weak);
  const fires = r?.state === "update_prompt" && (r?.confidence ?? 0) >= GLOBAL_THRESHOLD && !!r?.action?.keys;
  assert(!fires, "weak single-signal screen does not produce an actionable update verdict", JSON.stringify(r));
}

console.log("\n--- CRLF robustness: Windows pane output (\\r\\n) classifies identically to LF ---\n");
// Real Windows agent panes emit CRLF; the classifier must normalize internally
// so the verdict (state + confidence + actions) is byte-identical to the LF read.
const toCrlf = (s: string) => s.replace(/\r\n?/g, "\n").replace(/\n/g, "\r\n");
for (const f of ["update-prompt-skip-option2.txt", "update-prompt-skip-option3.txt", "auth-login.txt"]) {
  const lf = fixture(f).replace(/\r\n?/g, "\n");
  const crlf = toCrlf(lf);
  assert(crlf.includes("\r\n"), `${f}: CRLF variant actually contains \\r\\n`);
  const rLf = classify(plugin, lf);
  const rCrlf = classify(plugin, crlf);
  eq(rCrlf, rLf, `${f}: CRLF classification equals LF classification`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
