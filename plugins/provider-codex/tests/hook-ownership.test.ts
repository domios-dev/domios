/**
 * Codex hook ownership and reconciliation over its real event set.
 * Run: npx tsx plugins/provider-codex/tests/hook-ownership.test.ts
 *
 * The current fixture has SessionStart only, so reconciliation adds Stop.
 * The old marker fixture models the v1.14.9 install without writing to a real
 * provider directory.
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { CODETERM_CODEX_HOOK_MARKER_V1149, commandIsOwned, is1149CodexScript } from "../src/upgrade_from_1_14_9"; // rename-bridge
import { codexWindowsHookScript, isCurrentCodexHookScriptContent, isOwnedCodexHookScriptContent } from "../src/hooks";

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

const CODEX_MARKER = "domios-hook-v4";
/// Codex's own event set, as its real hooks file represents it.
const CODEX_EVENTS = ["SessionStart", "Stop"];

assert(commandIsOwned(`curl -H "X-CodeTerm-Hook: ${CODETERM_CODEX_HOOK_MARKER_V1149}"`), "the 1.14.9 hook marker is owned");
assert(!commandIsOwned(`curl -H "X-CodeTerm-Hook: ${CODETERM_CODEX_HOOK_MARKER_V1149}0"`), "a suffixed hook marker is not owned");
assert(is1149CodexScript(`rem ${CODETERM_CODEX_HOOK_MARKER_V1149}`), "the 1.14.9 script marker is owned");
assert(!is1149CodexScript("rem codeterm-profile-cleanup-v1"), "a cleanup label alone does not establish ownership"); // rename-bridge

const CURRENT_SESSION_START =
  'if [ -n "${DOMIOS_API_URL:-}" ] && [ -n "${DOMIOS_API_TOKEN:-}" ] && [ -n "${DOMIOS_TAB_ID:-}" ] && [ -n "${DOMIOS_SESSION_BINDING_NONCE:-}" ]; then '
  + 'curl -s -X POST "$DOMIOS_API_URL/api/tabs/$DOMIOS_TAB_ID/hook/session_start" '
  + "-H 'Content-Type: application/json' -H 'X-Domios-Agent-Provider: codex' "
  + '-H "X-Domios-Hook: domios-hook-v4" '
  + '-H "X-Domios-Binding-Nonce: ${DOMIOS_SESSION_BINDING_NONCE}" '
  + '-H "Authorization: Bearer ${DOMIOS_API_TOKEN}" --data-binary @- > /dev/null 2>&1 || true; fi';

const LEGACY_V1149_WINDOWS_HELPER = [
  "@echo off",
  "setlocal DisableDelayedExpansion",
  "rem " + CODETERM_CODEX_HOOK_MARKER_V1149, // rename-bridge
  "if not defined CODETERM_TAB_ID exit /b 0", // rename-bridge
  "if not defined CODETERM_API_URL exit /b 0", // rename-bridge
  "if not defined CODETERM_API_TOKEN exit /b 0", // rename-bridge
  "if not defined CODETERM_SESSION_BINDING_NONCE exit /b 0", // rename-bridge
  'set "hookEvent=%~1"',
  'set "hookMarker=%~2 %~3"',
  'if /I not "%hookMarker%"=="X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER_V1149 + '" exit /b 0', // rename-bridge
  'if /I "%hookEvent%"=="session_start" goto post_event',
  'if /I "%hookEvent%"=="stop" goto post_event',
  "exit /b 0",
  ":post_event",
  'set "apiBase=%CODETERM_API_URL%"', // rename-bridge
  'if "%apiBase:~-1%"=="/" set "apiBase=%apiBase:~0,-1%"',
  'set "responseFile=%TEMP%\\codeterm-hook-%CODETERM_TAB_ID%-%hookEvent%-%RANDOM%.tmp"', // rename-bridge
  'curl.exe --silent --show-error --max-time 30 --request POST "%apiBase%/api/tabs/%CODETERM_TAB_ID%/hook/%hookEvent%" --header "Content-Type: application/json" --header "X-CodeTerm-Agent-Provider: codex" --header "%hookMarker%" --header "X-CodeTerm-Binding-Nonce: %CODETERM_SESSION_BINDING_NONCE%" --header "Authorization: Bearer %CODETERM_API_TOKEN%" --output "%responseFile%" --data-binary @-', // rename-bridge
  'if exist "%responseFile%" del /F /Q "%responseFile%"',
  "exit /b 0",
  "",
].join("\r\n");

const LEGACY_V1149_SESSION_START =
  'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" ' // rename-bridge
  + "-H 'Content-Type: application/json' -H 'X-CodeTerm-Agent-Provider: codex' " // rename-bridge
  + '-H "X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER_V1149 + '" ' // rename-bridge
  + '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" ' // rename-bridge
  + '-H "Authorization: Bearer ${CODETERM_API_TOKEN}" --data-binary @-'; // rename-bridge

const ROG_PANE_SESSION_START =
  "curl -s -X POST http://localhost:7685/api/panes/$FOREIGN_PANE_ID/hook/session_start "
  + "-H 'Content-Type: application/json' -H 'X-Agent-Provider: codex' "
  + "--data-binary @- > /dev/null 2>&1 || true";
const FOREIGN_PANE_ROUTE = "curl http://localhost:7686/api/panes/$FOREIGN_PANE_ID/hook/session_start";

const FOREIGN_COMMAND = "~/bin/codex-audit.sh --event session_start";

const TEST_HOME = fs.mkdtempSync(path.join(os.tmpdir(), "domios-codex-hooks-"));
const HOOKS_PATH = path.posix.join(TEST_HOME, ".codex", "hooks.json");
const CONFIG_PATH = path.posix.join(TEST_HOME, ".codex", "config.toml");
const WINDOWS_HOOKS_PATH = path.win32.join(TEST_HOME, ".codex", "hooks.json");
const WINDOWS_CONFIG_PATH = path.win32.join(TEST_HOME, ".codex", "config.toml");
const LEGACY_WINDOWS_SCRIPT_PATH = path.win32.join(TEST_HOME, ".codex", "codeterm-session-hook.cmd"); // rename-bridge
const bundle = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");

type CodexHook = { type: string; command: string; commandWindows?: string; timeout?: number; _source?: string };
type CodexEntry = { matcher?: string; hooks: CodexHook[] };

function hookEntry(command: string, source?: string, matcher?: string): CodexEntry {
  const hook: CodexHook = { type: "command", command, timeout: 5 };
  if (source) hook._source = source;
  return matcher === undefined ? { hooks: [hook] } : { matcher, hooks: [hook] };
}

function loadCodex(files: Map<string, string>, env: Record<string, string> = {}, platform = "linux") {
  const host = {
    manifestJson: () => "null",
    homeDir: () => TEST_HOME,
    platform: () => platform,
    unixNowMs: () => 1_000_000,
    path: { equal: (a: string, b: string) => a === b },
    expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
    envGet: (name: string) => env[name] ?? null,
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
      || String(hook.command || "").indexOf(`X-Domios-Hook: ${CODEX_MARKER}`) !== -1));
}

// ── current_domios_hook_is_owned ──────────────────────────────────────────
{
  console.log("current_domios_hook_is_owned");
  const files = hooksFile({
    SessionStart: [
      hookEntry(CURRENT_SESSION_START, CODEX_MARKER, "startup|resume|clear|compact"),
    ],
  });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install over the installed set succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "one marked current SessionStart command remains");
  eq(ownedEntries(written.hooks.Stop).length, 1,
    "the missing Stop is installed, not treated as foreign absence");
  for (const event of CODEX_EVENTS) {
    const command = String(written.hooks[event][0].hooks[0].command);
    assert(command.indexOf("API_PORT") === -1, `${event}: no API port is embedded`);
    assert(command.indexOf("NUL") === -1, `${event}: no Windows reserved-name redirect`);
    assert(command.includes("X-Domios-Hook: domios-hook-v4"), `${event}: the Domios marker is present`);
  }
}

// ── v1_14_9_windows_helper_refreshes_once_on_startup ───────────────────────
{
  console.log("v1_14_9_windows_helper_refreshes_once_on_startup");
  const oldWindowsCommand = (event: string) =>
    `cmd.exe /d /s /c call "${LEGACY_WINDOWS_SCRIPT_PATH}" ${event} X-CodeTerm-Hook: ${CODETERM_CODEX_HOOK_MARKER_V1149}`; // rename-bridge
  const legacyStop =
    'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/stop" ' // rename-bridge
    + '-H "X-CodeTerm-Hook: ' + CODETERM_CODEX_HOOK_MARKER_V1149 + '"'; // rename-bridge
  const files = new Map<string, string>([
    [WINDOWS_HOOKS_PATH, JSON.stringify({ hooks: {
      SessionStart: [{ matcher: "startup|resume|clear|compact", hooks: [{
        type: "command",
        command: LEGACY_V1149_SESSION_START,
        commandWindows: oldWindowsCommand("session_start"),
        timeout: 60,
        _source: CODETERM_CODEX_HOOK_MARKER_V1149,
      }] }],
      Stop: [{ hooks: [{
        type: "command",
        command: legacyStop,
        commandWindows: oldWindowsCommand("stop"),
        timeout: 60,
        _source: CODETERM_CODEX_HOOK_MARKER_V1149,
      }] }],
    } })],
    [WINDOWS_CONFIG_PATH, 'model = "gpt-5.6"\n'],
    [LEGACY_WINDOWS_SCRIPT_PATH, LEGACY_V1149_WINDOWS_HELPER],
  ]);
  assert(isOwnedCodexHookScriptContent(LEGACY_V1149_WINDOWS_HELPER),
    "the exact 1.14.9 helper marker remains owned");
  assert(!isCurrentCodexHookScriptContent(LEGACY_V1149_WINDOWS_HELPER),
    "the 1.14.9 helper is stale against the current generator");

  const p = loadCodex(files, {}, "windows");
  const before = p.checkIntegration();
  assert(before.hooks_owned === true, "the old marker still identifies the installed hook as owned");
  assert(before.hook_contract_current === false, "the old helper and commands read as stale");
  const refresh = p.ensureIntegrationOnStartup(7686);
  assert(refresh.ok === true, "startup refreshes the previously consented owned install", JSON.stringify(refresh));

  const written = JSON.parse(files.get(WINDOWS_HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "refresh keeps exactly one SessionStart entry");
  eq(written.hooks.Stop.length, 1, "refresh keeps exactly one Stop entry");
  assert(written.hooks.SessionStart[0].hooks[0].command.includes("X-Domios-Hook: domios-hook-v4"),
    "SessionStart is replaced by the current marker");
  assert(written.hooks.Stop[0].hooks[0].command.includes("X-Domios-Hook: domios-hook-v4"),
    "Stop is replaced by the current marker");
  eq(files.get(LEGACY_WINDOWS_SCRIPT_PATH), codexWindowsHookScript(),
    "refresh writes exactly the current helper generator output");
  assert(p.checkIntegration().hook_contract_current === true, "the refreshed install is current");

  const afterFirstRefresh = Array.from(files.entries());
  const secondRefresh = p.ensureIntegrationOnStartup(7686);
  assert(secondRefresh.changed === false, "the next startup performs no second refresh");
  eq(Array.from(files.entries()), afterFirstRefresh, "the second startup writes no provider file");
}

// ── v1_14_9_marker_is_replaced_in_a_release ────────────────────────────────
{
  console.log("v1_14_9_marker_is_replaced_in_a_release");
  const files = hooksFile({
    SessionStart: [
      hookEntry(LEGACY_V1149_SESSION_START, "legacy-source-key", "startup|resume|clear|compact"),
    ],
  });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "release install replaces the exact old marker");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "the old marked entry is replaced, not appended to");
  assert(String(written.hooks.SessionStart[0].hooks[0].command).includes("X-Domios-Hook: domios-hook-v4"),
    "the new marker is installed");
  assert(!String(written.hooks.SessionStart[0].hooks[0].command).includes("X-CodeTerm-Hook:"), // rename-bridge
    "the previous marker is absent from the replacement");
}

// ── dev_with_v1_14_9_generation_does_not_write_the_temp_home ──────────────
{
  console.log("dev_with_v1_14_9_generation_does_not_write_the_temp_home");
  const files = hooksFile({
    SessionStart: [hookEntry(LEGACY_V1149_SESSION_START, undefined, "startup|resume|clear|compact")],
  });
  const before = Array.from(files.entries());
  const p = loadCodex(files, { DOMIOS_INSTANCE: "split-oracle" });
  assert(p.installIntegration(7686, { consent: true }).ok === true, "DEV leaves the old generation alone");
  eq(Array.from(files.entries()), before, "the temporary home files remain byte-identical");
}

// ── unmarked_pane_generation_stays_foreign ─────────────────────────────────
{
  const foreign = hookEntry(ROG_PANE_SESSION_START, "legacy-pane-record", "startup|resume|clear|compact");
  const files = hooksFile({ SessionStart: [foreign] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install beside an unmarked pane hook succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart[0], foreign, "unmarked pane-era content stays byte-identical");
  eq(written.hooks.SessionStart.length, 2, "the unmarked pane-era command is not claimed");
}

{
  const foreign = hookEntry(FOREIGN_PANE_ROUTE, "acme-audit", "startup");
  const files = hooksFile({ SessionStart: [foreign] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install alongside a pane-route lookalike succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart[0], foreign, "the similar pane route stays byte-identical");
}

// ── route_and_provider_headers_do_not_claim_a_foreign_hook ─────────────────
{
  console.log("route_and_provider_headers_do_not_claim_a_foreign_hook");
  const foreign = hookEntry(
    'curl -s -X POST "$DOMIOS_API_URL/api/tabs/$DOMIOS_TAB_ID/hook/session_start" '
    + "-H 'X-Domios-Agent-Provider: codex' --data-binary @-",
    "acme-audit",
    "startup",
  );
  const files = hooksFile({ SessionStart: [foreign] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install alongside a route-shaped foreign hook succeeds");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart[0], foreign, "route and provider headers do not claim foreign content");
  eq(ownedEntries(written.hooks.SessionStart).length, 1, "the marked Domios command remains independently owned");
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
    assert(String(written.hooks[event][0].hooks[0].command).includes(`X-Domios-Hook: ${CODEX_MARKER}`),
      `${event}: the ownership marker lives in the command`);
  }
}

// ── command_marker_is_the_ownership_key ─────────────────────────────────────
{
  console.log("command_marker_is_the_ownership_key");
  const markerCommand =
    LEGACY_V1149_SESSION_START;
  const files = hooksFile({ SessionStart: [hookEntry(markerCommand, "acme-audit", "startup")] });
  const p = loadCodex(files);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "install recognizes the in-command owner marker");
  const written = JSON.parse(files.get(HOOKS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "the marker-owned hook is replaced without a duplicate");
  assert(String(written.hooks.SessionStart[0].hooks[0].command).includes(`X-Domios-Hook: ${CODEX_MARKER}`),
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
      hookEntry(CURRENT_SESSION_START, CODEX_MARKER, "startup|resume|clear|compact"),
      hookEntry(CURRENT_SESSION_START, CODEX_MARKER, "startup|resume|clear|compact"),
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
    eq(ownedEntries(written.hooks[event]).length, 1, `${event}: exactly one Domios entry`);
    const command = String(ownedEntries(written.hooks[event])[0].hooks[0].command);
    assert(command.indexOf("localhost") === -1 && command.indexOf("7685") === -1
      && command.indexOf("7686") === -1 && command.indexOf("API_PORT") === -1,
      `${event}: instance-agnostic, no port baked in`);
    assert(command.indexOf("${DOMIOS_API_TOKEN}") !== -1 && command.indexOf("Bearer ct_") === -1,
      `${event}: the token is read from the environment, never embedded`);
  }
  eq(written.hooks.SessionStart.length, 2, "SessionStart keeps the foreign entry plus exactly one of ours");
  eq(written.hooks.SessionStart[0], foreign, "the foreign entry is byte-identical");
  eq(written.hooks.Stop.length, 1, "Stop carries exactly one entry");
}

console.log(`\n${passed} passed, ${failed} failed`);
fs.rmSync(TEST_HOME, { recursive: true, force: true });
if (failed > 0) process.exit(1);
