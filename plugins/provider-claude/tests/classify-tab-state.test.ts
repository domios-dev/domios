/**
 * Claude tab-state classifier tests (AS3 — Case B: rate-limit confirm-wait +
 * scheduled resume; AS4 — clarifying question surface + auth guard).
 * Run: npx tsx plugins/provider-claude/tests/classify-tab-state.test.ts
 *
 * Loads the built QuickJS bundle (plugin.js) and exercises classifyTabState
 * against golden fixtures: a rate-limit menu must classify as `rate_limited`
 * above threshold, navigate to the wait option then Enter, and schedule a
 * `continue your work` resume at the parsed reset time + buffer; a clarifying
 * question must classify as `clarifying_question` and SURFACE the question with
 * NO key action; an auth/login screen must classify as `waiting_on_auth` with no
 * auto-press; a negative corpus of normal output must never fire.
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

// Mirror the core reactor gate (src-tauri/src/agent/state_reactor.rs).
const GLOBAL_THRESHOLD = 0.8;
// Pin "now" so reset-time parsing is deterministic: 2026-06-19T08:00:00Z.
const NOW_MS = Date.parse("2026-06-19T08:00:00Z");

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

function loadClaude() {
  const pluginPath = path.join(here, "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const host = {
    manifestJson: () => "null",
    homeDir: () => "/tmp/home",
    platform: () => "linux",
    envGet: () => null,
    unixNowMs: () => NOW_MS,
    expandHome: (v: string) => v,
    fileExists: () => false,
    removeFile: () => true,
    writeFile: () => true,
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
    agentType: "claude",
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

const plugin = loadClaude();

assert(typeof plugin.classifyTabState === "function", "classifyTabState is exported");

console.log("\n--- positive: rate-limit menu, Stop-and-wait is the default highlight ---\n");
{
  const r = classify(plugin, fixture("rate-limit-menu.txt"));
  eq(r?.state, "rate_limited", "state is rate_limited");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  // Default highlight is already the wait option → a bare Enter confirms it.
  eq(r?.action?.keys, ["enter"], "confirms the wait option (already highlighted)");
  assert(
    Array.isArray(r?.action?.keys) && r.action.keys[r.action.keys.length - 1] === "enter",
    "action ends with enter",
  );
  // Reset 4:20pm parsed → a delayed resume scheduled after reset + buffer.
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

console.log("\n--- positive: wait option NOT the default highlight → navigate to it ---\n");
{
  const r = classify(plugin, fixture("rate-limit-wait-not-default.txt"));
  eq(r?.state, "rate_limited", "state is rate_limited");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  // Highlight is option 2 (Upgrade); wait is option 1 → move up once, then enter.
  eq(r?.action?.keys, ["up", "enter"], "navigates up off the Upgrade highlight to the wait option");
  // Cardinal-sin guard: never a bare Enter on a foreign (Upgrade) highlight.
  assert(
    Array.isArray(r?.action?.keys) && r.action.keys.length > 1 && r.action.keys[0] !== "enter",
    "action moves off the default highlight before confirming",
  );
}

console.log("\n--- positive: rate-limit menu, reset time unparseable → confirm + SURFACE (no schedule) ---\n");
{
  const r = classify(plugin, fixture("rate-limit-no-reset-time.txt"));
  eq(r?.state, "rate_limited", "state is rate_limited");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  eq(r?.action?.keys, ["enter"], "still confirms the wait (safe)");
  assert(!r?.action?.delayedResume, "no blind schedule when reset time didn't parse", JSON.stringify(r?.action));
  eq(r?.action?.surface?.kind, "rate_limit", "surfaces for a manual resume instead");
}

console.log("\n--- positive: clarifying question → surface only, NEVER auto-answered ---\n");
{
  const r = classify(plugin, fixture("clarifying-question.txt"));
  eq(r?.state, "clarifying_question", "state is clarifying_question");
  assert((r?.confidence ?? 0) >= GLOBAL_THRESHOLD, "confidence >= threshold", `conf=${r?.confidence}`);
  assert(!r?.action?.keys, "clarifying question carries NO key action (never auto-answered)", JSON.stringify(r?.action));
  eq(r?.action?.surface?.kind, "clarifying_question", "surfaces a clarifying_question card");
  const qs = r?.data?.questions;
  assert(Array.isArray(qs) && qs.length > 0, "extracts the question(s)", JSON.stringify(qs));
  assert(
    Array.isArray(qs) && qs.some((q: string) => /which database/i.test(q)),
    "surfaced question is the actual prompt",
    JSON.stringify(qs),
  );
}

console.log("\n--- positive: structural pending prompt and active terminal choice surface once ---\n");
for (const f of ["structured-pending-prompt.txt", "active-terminal-choice.txt"]) {
  const r = classify(plugin, fixture(f));
  eq(r?.state, "clarifying_question", `${f}: state is clarifying_question`);
  assertSingleInputSurface(r, "clarifying_question", f);
}

console.log("\n--- auth guard: login screen → waiting_on_auth, NEVER auto-acted ---\n");
{
  const r = classify(plugin, fixture("auth-login.txt"));
  eq(r?.state, "waiting_on_auth", "auth/login classified as waiting_on_auth");
  assertSingleInputSurface(r, "auth_prompt", "auth/login");
}

console.log("\n--- stale auth prose at ready or streaming tail is neither Question nor Auth ---\n");
for (const f of ["stale-auth-ready-composer.txt", "stale-auth-streaming.txt"]) {
  assertNoInputRequest(classify(plugin, fixture(f)), f);
}
{
  const prose = "We discussed sign in, login, OAuth authentication, API keys, and authorization codes.";
  assertNoInputRequest(classify(plugin, prose), "prose similarity alone");
}

console.log("\n--- negatives: normal/resolved output never fires (no false positives) ---\n");
for (const f of [
  "normal-working.txt",
  "tool-call-list.txt",
  "diff-and-shell.txt",
  "prose-limit-mention.txt",
  "answered-clarifying-question.txt",
]) {
  const r = classify(plugin, fixture(f));
  const state = r?.state ?? "unknown";
  assert(state !== "rate_limited", `${f}: not classified rate_limited`, `state=${state}`);
  assert(state !== "clarifying_question", `${f}: not classified clarifying_question`, `state=${state}`);
  assert(state !== "waiting_on_auth", `${f}: not classified waiting_on_auth`, `state=${state}`);
  assert(!r?.action?.keys, `${f}: no auto-key action`, JSON.stringify(r?.action));
}

console.log("\n--- ensemble: a single weak signal must NOT reach threshold ---\n");
{
  // not actionable (no wait option to navigate to).
  const weak = "Heads up: your usage limit resets 5pm today. Continuing for now.\n";
  const r = classify(plugin, weak);
  const fires = r?.state === "rate_limited" && (r?.confidence ?? 0) >= GLOBAL_THRESHOLD && !!r?.action?.keys;
  assert(!fires, "weak single-signal screen does not produce an actionable rate-limit verdict", JSON.stringify(r));
}

console.log("\n--- CRLF robustness: Windows tab output (\\r\\n) classifies identically to LF ---\n");
// so the verdict (state + confidence + actions) is byte-identical to the LF read.
const toCrlf = (s: string) => s.replace(/\r\n?/g, "\n").replace(/\n/g, "\r\n");
for (const f of ["rate-limit-menu.txt", "rate-limit-wait-not-default.txt", "clarifying-question.txt", "auth-login.txt"]) {
  const lf = fixture(f).replace(/\r\n?/g, "\n");
  const crlf = toCrlf(lf);
  assert(crlf.includes("\r\n"), `${f}: CRLF variant actually contains \\r\\n`);
  const rLf = classify(plugin, lf);
  const rCrlf = classify(plugin, crlf);
  eq(rCrlf, rLf, `${f}: CRLF classification equals LF classification`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
