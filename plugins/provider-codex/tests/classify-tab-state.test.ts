/**
 * Codex tab-state classifier tests (AS2 — Case A: self-update consent prompt).
 * Run: npx tsx plugins/provider-codex/tests/classify-tab-state.test.ts
 *
 * Loads the built QuickJS bundle (plugin.js) and exercises classifyTabState
 * against golden fixtures: the codex update prompt must classify as
 * `update_prompt` with separate install and decline actions above the reactor's
 * confidence threshold; an auth/login screen must classify as `waiting_on_auth`
 * with NO auto-press; a negative corpus of normal output must never fire.
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

// Mirror the core reactor gate (src-tauri/src/agent/state_reactor.rs).
const GLOBAL_THRESHOLD = 0.8;
// Pin "now" so reset-time parsing is deterministic: 2026-06-25 22:38 local (same evening as 11:07 PM reset).
const NOW_MS = new Date(2026, 5, 25, 22, 38, 0, 0).getTime();

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
    unixNowMs: () => NOW_MS,
    log: () => {},
    shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
    fs: { fileExists: () => false, readFile: () => null, readJson: () => null, writeJson: () => true, writeFile: () => true },
  };
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

function fixture(name: string): string {
  return fs.readFileSync(path.join(here, "fixtures", "tab-state", name), "utf8");
}

function classify(plugin: any, screenText: string) {
  return plugin.classifyTabState({
    tabId: "tab-1",
    agentType: "codex",
    screenText,
  });
}

function assertNoInputRequest(r: any, name: string) {
  assert(r?.state !== "waiting_on_auth", `${name}: not Auth`, JSON.stringify(r));
  assert(r?.state !== "clarifying_question", `${name}: not Question`, JSON.stringify(r));
  assert(r?.action?.surface?.kind !== "auth_prompt", `${name}: no auth surface`, JSON.stringify(r));
  assert(r?.action?.surface?.kind !== "clarifying_question", `${name}: no question surface`, JSON.stringify(r));
}

function assertSingleInputSurface(r: any, kind: string, name: string) {
  eq(r?.action?.surface?.kind, kind, `${name}: carries one ${kind} surface`);
  assert(!r?.action?.keys, `${name}: carries no key action`, JSON.stringify(r?.action));
}

const plugin = loadCodex();

assert(typeof plugin.classifyTabState === "function", "classifyTabState is exported");

console.log("\n--- positive: update prompt, Skip is option 2 ---\n");
{
  const r = classify(plugin, fixture("update-prompt-skip-option2.txt"));
  eq(r?.state, "update_prompt", "state is update_prompt");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  eq(r?.action?.providerUpdate?.installKeys, ["enter"], "install selects the highlighted update action");
  eq(r?.action?.providerUpdate?.declineKeys, ["down", "enter"], "decline navigates one down to Skip, then enter");
  // highlight "Update now". The action must move OFF option 1 before enter.
  assert(
    Array.isArray(r?.action?.providerUpdate?.declineKeys) && r.action.providerUpdate.declineKeys.indexOf("down") !== -1,
    "decline moves off the default 'Update now' highlight",
  );
  assert(
    Array.isArray(r?.action?.providerUpdate?.declineKeys) && r.action.providerUpdate.declineKeys[r.action.providerUpdate.declineKeys.length - 1] === "enter",
    "decline ends with enter",
  );
}

console.log("\n--- positive: update prompt, Skip is option 3 (dynamic navigation) ---\n");
{
  const r = classify(plugin, fixture("update-prompt-skip-option3.txt"));
  eq(r?.state, "update_prompt", "state is update_prompt");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  eq(r?.action?.providerUpdate?.installKeys, ["enter"], "install selects the highlighted update action");
  eq(r?.action?.providerUpdate?.declineKeys, ["down", "down", "enter"], "decline navigates two downs to Skip (option 3), then enter");
}

console.log("\n--- auth guard: login screen → waiting_on_auth, NEVER auto-acted ---\n");
{
  const r = classify(plugin, fixture("auth-login.txt"));
  eq(r?.state, "waiting_on_auth", "auth/login classified as waiting_on_auth");
  assertSingleInputSurface(r, "auth_prompt", "auth/login");
}

console.log("\n--- positive: structural pending prompt and active terminal choice surface once ---\n");
for (const f of ["structured-pending-prompt.txt", "active-terminal-choice.txt"]) {
  const r = classify(plugin, fixture(f));
  eq(r?.state, "clarifying_question", `${f}: state is clarifying_question`);
  assertSingleInputSurface(r, "clarifying_question", f);
}

console.log("\n--- stale auth prose at ready or streaming tail is neither Question nor Auth ---\n");
for (const f of ["stale-auth-ready-composer.txt", "stale-auth-streaming.txt"]) {
  assertNoInputRequest(classify(plugin, fixture(f)), f);
}
{
  const prose = "We discussed sign in, login, OAuth authentication, API keys, and authorization codes.";
  assertNoInputRequest(classify(plugin, prose), "prose similarity alone");
}

console.log("\n--- positive: usage limit notice → rate_limited + scheduled resume ---\n");
{
  const r = classify(plugin, fixture("rate-limit-try-again.txt"));
  eq(r?.state, "rate_limited", "state is rate_limited");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  assert(!r?.action?.keys, "no key action on plain-text limit notice", JSON.stringify(r?.action));
  const resume = r?.action?.delayedResume;
  assert(!!resume, "carries a delayed resume", JSON.stringify(r?.action));
  eq(resume?.text, "continue your work", "resume text is 'continue your work'");
  eq(resume?.thenKeys, ["enter"], "resume submits with enter");
  assert(typeof r?.data?.resetAt === "string", "data.resetAt is set", JSON.stringify(r?.data));
  const resetMs = Date.parse(r?.data?.resetAt ?? "");
  assert(!Number.isNaN(resetMs) && resetMs > NOW_MS, "resetAt parses and is in the future", r?.data?.resetAt);
  const resumeMs = Date.parse(resume?.at ?? "");
  assert(resumeMs > resetMs, "resume fires strictly after reset (buffer)", `${resume?.at} vs ${r?.data?.resetAt}`);
}

console.log("\n--- positive: usage limit without reset time → surface only (no schedule) ---\n");
{
  const r = classify(plugin, fixture("rate-limit-no-reset-time.txt"));
  eq(r?.state, "rate_limited", "state is rate_limited");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  assert(!r?.action?.delayedResume, "no blind schedule when reset time didn't parse", JSON.stringify(r?.action));
  eq(r?.action?.surface?.kind, "rate_limit", "surfaces for a manual resume instead");
}

console.log("\n--- negatives: normal output never fires (no false positives) ---\n");
for (const f of ["normal-working.txt", "tool-call-list.txt", "diff-and-shell.txt"]) {
  const r = classify(plugin, fixture(f));
  const state = r?.state ?? "unknown";
  assert(state !== "update_prompt", `${f}: not classified update_prompt`, `state=${state}`);
  assert(state !== "waiting_on_auth", `${f}: not classified waiting_on_auth`, `state=${state}`);
  assert(state !== "rate_limited", `${f}: not classified rate_limited`, `state=${state}`);
  assert(!r?.action?.keys, `${f}: no auto-key action`, JSON.stringify(r?.action));
}

console.log("\n--- picker guard: model picker must not classify as rate_limited ---\n");
{
  const picker = fs.readFileSync(path.join(here, "fixtures", "model-picker.txt"), "utf8");
  const r = classify(plugin, picker);
  const state = r?.state ?? "unknown";
  assert(state !== "rate_limited", "model picker is not rate_limited", `state=${state}`);
  assert(!r?.action?.delayedResume, "model picker gets no delayed resume", JSON.stringify(r?.action));
}

console.log("\n--- ensemble: a single weak signal must NOT reach threshold ---\n");
{
  // threshold (this is the diff-and-shell npm-notice shape, isolated).
  const weak = "Some log line: New version available! 1.0.0 -> 1.1.0\nnothing actionable here\n";
  const r = classify(plugin, weak);
  const fires = r?.state === "update_prompt" && (r?.confidence ?? 0) >= GLOBAL_THRESHOLD && !!r?.action?.keys;
  assert(!fires, "weak single-signal screen does not produce an actionable update verdict", JSON.stringify(r));
  const rlWeak = "Heads up: your usage limit resets 5pm today. Continuing for now.\n";
  const r2 = classify(plugin, rlWeak);
  const rlFires = r2?.state === "rate_limited" && (r2?.confidence ?? 0) >= GLOBAL_THRESHOLD;
  assert(!rlFires, "casual limit mention without try-again notice does not fire", JSON.stringify(r2));
}

console.log("\n--- CRLF robustness: Windows tab output (\\r\\n) classifies identically to LF ---\n");
// so the verdict (state + confidence + actions) is byte-identical to the LF read.
const toCrlf = (s: string) => s.replace(/\r\n?/g, "\n").replace(/\n/g, "\r\n");
for (const f of ["update-prompt-skip-option2.txt", "update-prompt-skip-option3.txt", "auth-login.txt", "rate-limit-try-again.txt"]) {
  const lf = fixture(f).replace(/\r\n?/g, "\n");
  const crlf = toCrlf(lf);
  assert(crlf.includes("\r\n"), `${f}: CRLF variant actually contains \\r\\n`);
  const rLf = classify(plugin, lf);
  const rCrlf = classify(plugin, crlf);
  eq(rCrlf, rLf, `${f}: CRLF classification equals LF classification`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
