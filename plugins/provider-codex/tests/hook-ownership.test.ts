/**
 * Codex hook ownership and reconciliation over its real event set.
 * Run: npx tsx plugins/provider-codex/tests/hook-ownership.test.ts
 *
 * The `current` fixture is the verbatim ~/.codex/hooks.json entry read from
 * BLACKSTAR: SessionStart only, no Stop, and a port literal in the command.
 * That missing Stop is the provider asymmetry — reconciliation must add it,
 * never treat its absence as foreign content. The `legacy` fixture is the
 * pane-era form reported on ROG in issue 476.
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

const CODEX_MARKER = "codeterm-codex-session-binding-v1";
/// Codex's own event set, as its real hooks file represents it.
const CODEX_EVENTS = ["SessionStart", "Stop"];

const BLACKSTAR_SESSION_START =
  "curl -s -X POST http://localhost:${CODETERM_API_PORT:-7686}/api/tabs/$CODETERM_TAB_ID/hook/session_start "
  + "-H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' "
  + '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" '
  + '-H "Authorization: Bearer ${CODETERM_API_TOKEN}" --data-binary @- > /dev/null 2>&1 || true';

const LEGACY_ENV_SESSION_START =
  'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then '
  + 'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" '
  + "-H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' "
  + '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" '
  + '-H "Authorization: Bearer ${CODETERM_API_TOKEN}" '
  + "--data-binary @- > /dev/null 2>&1 || true; fi";

const ROG_PANE_SESSION_START =
  "curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start "
  + "-H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' "
  + "--data-binary @- > /dev/null 2>&1 || true";

const FOREIGN_COMMAND = "~/bin/codex-audit.sh --event session_start";

const HOOKS_PATH = "/tmp/home/.codex/hooks.json";
const CONFIG_PATH = "/tmp/home/.codex/config.toml";
const bundle = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");

type CodexHook = { type: string; command: string; commandWindows?: string; timeout?: number; _source?: string };
type CodexEntry = { matcher?: string; hooks: CodexHook[] };

function hookEntry(command: string, source?: string, matcher?: string): CodexEntry {
  const hook: CodexHook = { type: "command", command, timeout: 5 };
  if (source) hook._source = source;
  return matcher === undefined ? { hooks: [hook] } : { matcher, hooks: [hook] };
}

function loadCodex(files: Map<string, string>) {
  const host = {
    manifestJson: () => "null",
    homeDir: () => "/tmp/home",
    platform: () => "linux",
    unixNowMs: () => 1_000_000,
    path: { equal: (a: string, b: string) => a === b },
    expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
    envGet: () => null,
    shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
    log: () => {},
    fs: {
      fileExists: (p: string) => files.has(p),
      readFile: (p: string) => files.get(p) ?? null,
      readJson: (p: string) => {
        const raw = files.get(p);
        return raw === undefined ? null : JSON.parse(raw);
      },
      writeFile: (p: string, v: string) => { files.set(p, v); return true; },
      writeJson: (p: string, v: unknown) => { files.set(p, JSON.stringify(v, null, 2)); return true; },
      removeFile: (p: string) => files.delete(p),
      writeTempFile: () => "/tmp/ct-task",
    },
  };
  const source = fs.readFileSync(bundle, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: bundle }).runInNewContext(context);
  return context.module.exports.default;
}

function hooksFile(hooks: Record<string, CodexEntry[]>): Map<string, string> {
  return new Map([
    [HOOKS_PATH, JSON.stringify({ hooks })],
    [CONFIG_PATH, 'model = "gpt-5.6"\n'],
  ]);
}

function ownedEntries(entries: CodexEntry[] | undefined): CodexEntry[] {
  return (entries || []).filter((entry) =>
    (entry.hooks || []).some((hook) =>
      hook._source === CODEX_MARKER
      || String(hook.command || "").indexOf(`X-CodeTerm-Hook: ${CODEX_MARKER}`) !== -1));
}

// ── current_unmarked_codeterm_hook_is_owned ────────────────────────────────
{
  console.log("current_unmarked_codeterm_hook_is_owned");
  const files = hooksFile({
    SessionStart: [
      hookEntry(BLACKSTAR_SESSION_START, CODEX_MARKER, "startup|resume|clear|compact"),
      hookEntry(BLACKSTAR_SESSION_START, undefined, "startup|resume|clear|compact"),
      hookEntry(LEGACY_ENV_SESSION_START, undefined, "startup|resume|clear|compact"),
    ],
  });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install over the installed set succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "markerless exact legacy commands collapse into one");
  eq(ownedEntries(written.hooks.Stop).length, 1,
    "the missing Stop is installed, not treated as foreign absence");
  for (const event of CODEX_EVENTS) {
    const command = String(written.hooks[event][0].hooks[0].command);
    assert(command.indexOf("CODETERM_API_PORT") === -1, `${event}: the port literal is gone`);
    assert(command.indexOf("NUL") === -1, `${event}: no Windows reserved-name redirect`);
  }
}

// ── legacy_v2_codeterm_hook_is_owned ───────────────────────────────────────
{
  console.log("legacy_v2_codeterm_hook_is_owned");
  const files = hooksFile({
    SessionStart: [hookEntry(ROG_PANE_SESSION_START, CODEX_MARKER, "startup|resume|clear|compact")],
  });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install over the pane-era entry succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "the pane-era entry is replaced, not appended to");
  assert(JSON.stringify(written.hooks).indexOf("CODETERM_PANE_ID") === -1,
    "no pane-era command survives the migration");
}

// ── route_and_provider_headers_do_not_claim_a_foreign_hook ─────────────────
{
  console.log("route_and_provider_headers_do_not_claim_a_foreign_hook");
  const foreign = hookEntry(
    'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" '
    + "-H 'X-CodeTerm-Agent-Provider: codex' --data-binary @-",
    "acme-audit",
    "startup",
  );
  const files = hooksFile({ SessionStart: [foreign] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install alongside a route-shaped foreign hook succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart[0], foreign, "route and provider headers do not claim foreign content");
  eq(ownedEntries(written.hooks.SessionStart).length, 1, "the marked CodeTerm command remains independently owned");
}

// ── command_marker_survives_source_key_loss ─────────────────────────────────
{
  console.log("command_marker_survives_source_key_loss");
  const files = hooksFile({});
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "initial marked install succeeds");
  const installed = JSON.parse(files.get(HOOKS_PATH) as string);
  for (const event of CODEX_EVENTS) {
    for (const entry of installed.hooks[event]) {
      for (const hook of entry.hooks) delete hook._source;
    }
  }
  files.set(HOOKS_PATH, JSON.stringify(installed));
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install survives loss of the sibling source key");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  for (const event of CODEX_EVENTS) {
    eq(written.hooks[event].length, 1, `${event}: the command marker prevents duplicate installation`);
    assert(String(written.hooks[event][0].hooks[0].command).includes(`X-CodeTerm-Hook: ${CODEX_MARKER}`),
      `${event}: the ownership marker lives in the command`);
  }
}

// ── command_marker_is_the_ownership_key ─────────────────────────────────────
{
  console.log("command_marker_is_the_ownership_key");
  const markerCommand =
    'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" '
    + '-H "X-CodeTerm-Hook: codeterm-codex-session-binding-v1"';
  const files = hooksFile({ SessionStart: [hookEntry(markerCommand, "acme-audit", "startup")] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install recognizes the in-command owner marker");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "the marker-owned hook is replaced without a duplicate");
  assert(String(written.hooks.SessionStart[0].hooks[0].command).includes(`X-CodeTerm-Hook: ${CODEX_MARKER}`),
    "the replacement keeps the marker in the command");
}

// ── foreign_hook_is_not_owned ──────────────────────────────────────────────
{
  console.log("foreign_hook_is_not_owned");
  const foreign = hookEntry(FOREIGN_COMMAND, "acme-audit", "startup");
  const files = hooksFile({ SessionStart: [foreign] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install alongside a foreign hook succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 2, "the foreign entry survives beside ours");
  eq(written.hooks.SessionStart[0], foreign, "the foreign entry is byte-identical");
  eq(ownedEntries(written.hooks.SessionStart).length, 1, "exactly one entry is ours");
}

// ── reconcile_current_hooks_twice_keeps_one_entry_per_event ────────────────
{
  console.log("reconcile_current_hooks_twice_keeps_one_entry_per_event");
  const foreign = hookEntry(FOREIGN_COMMAND, "acme-audit", "startup");
  const files = hooksFile({
    SessionStart: [
      foreign,
      hookEntry(BLACKSTAR_SESSION_START, CODEX_MARKER, "startup|resume|clear|compact"),
      hookEntry(ROG_PANE_SESSION_START, undefined, "startup|resume|clear|compact"),
    ],
  });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "first reconciliation succeeds");
  const first = files.get(HOOKS_PATH) as string;
  assert(p.installIntegration(7686, { consent: true }).ok === true, "second reconciliation succeeds");
  const second = files.get(HOOKS_PATH) as string;
  eq(first, second, "a second reconciliation writes nothing new");

  const written = JSON.parse(second);
  for (const event of CODEX_EVENTS) {
    eq(ownedEntries(written.hooks[event]).length, 1, `${event}: exactly one CodeTerm entry`);
    const command = String(ownedEntries(written.hooks[event])[0].hooks[0].command);
    assert(command.indexOf("localhost") === -1 && command.indexOf("7685") === -1
      && command.indexOf("7686") === -1 && command.indexOf("CODETERM_API_PORT") === -1,
      `${event}: instance-agnostic, no port baked in`);
    assert(command.indexOf("${CODETERM_API_TOKEN}") !== -1 && command.indexOf("Bearer ct_") === -1,
      `${event}: the token is read from the environment, never embedded`);
  }
  eq(written.hooks.SessionStart.length, 2, "SessionStart keeps the foreign entry plus exactly one of ours");
  eq(written.hooks.SessionStart[0], foreign, "the foreign entry is byte-identical");
  eq(written.hooks.Stop.length, 1, "Stop carries exactly one entry");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
