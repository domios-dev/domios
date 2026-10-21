/**
 * Claude hook ownership and reconciliation, against the hook forms observed on
 * real machines.
 * Run: npx tsx plugins/provider-claude/tests/hook-ownership.test.ts
 *
 * The `current` fixtures are the verbatim ~/.claude/settings.json hook set read
 * from BLACKSTAR, where Claude Code's own serializer has already stripped every
 * `_source`. The `legacy v2` fixtures are the pane-era form reported on ROG in
 * issue 476 (`${CODETERM_API_PORT:-7685}` + `/api/panes/$CODETERM_PANE_ID/`).
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

const MARKER_HEADER = "X-CodeTerm-Hook: codeterm-hook-v3";
const EVENTS = ["PermissionRequest", "Notification", "Stop", "SessionStart"];

const GUARD = 'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then ';
const AUTH = '-H "Authorization: Bearer $CODETERM_API_TOKEN"';
const NONCE = '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';

const CURRENT_PERMISSION = GUARD
  + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/permission" '
  + "-H 'Content-Type: application/json' " + AUTH + " " + NONCE + " --data-binary @- || true; fi";
const CURRENT_STOP = GUARD
  + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/stop" '
  + "-H 'Content-Type: application/json' " + AUTH + " -d '{}' || true; fi";
const CURRENT_PRECOMPACT = GUARD
  + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/mem/flush" '
  + "-H 'Content-Type: application/json' " + AUTH
  + ' -d \'{"tab":"\'"$CODETERM_TAB_ID"\'","title":"pre-compact"}\' || true; fi';
const CURRENT_SESSION_START = GUARD
  + 'curl -s -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" '
  + "-H 'Content-Type: application/json' " + AUTH + " " + NONCE + " --data-binary @- || true; fi";

const LEGACY_V2_SESSION_START =
  "curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/session_start "
  + "-H 'Content-Type: application/json' " + NONCE + " -d '{}' > /dev/null 2>&1 || true";
const LEGACY_V2_STOP =
  "curl -s -X POST http://localhost:${CODETERM_API_PORT:-7685}/api/panes/$CODETERM_PANE_ID/hook/stop "
  + "-H 'Content-Type: application/json' -d '{}' > /dev/null 2>&1 || true";
const FOREIGN_PANE_ROUTE = "curl http://localhost:7686/api/panes/$CODETERM_PANE_ID/hook/session_start";

const FOREIGN_LOCAL_HTTP = 'curl -s -X POST http://localhost:7686/api/telemetry/session -d \'{"kind":"start"}\'';
const FOREIGN_SCRIPT = "~/bin/my-audit-hook.sh --event stop";

type Hook = { type: string; command: string; timeout?: number; _source?: string };
type Entry = { matcher?: string; hooks: Hook[] };

function entry(matcher: string, command: string, source?: string): Entry {
  const hook: Hook = { type: "command", command, timeout: 5000 };
  if (source) hook._source = source;
  return { matcher, hooks: [hook] };
}

function currentUnmarked(event: string): Entry {
  if (event === "Notification") return entry("permission_prompt", CURRENT_PERMISSION);
  if (event === "PermissionRequest") return entry("*", CURRENT_PERMISSION);
  if (event === "Stop") return entry("", CURRENT_STOP);
  if (event === "PreCompact") return entry("", CURRENT_PRECOMPACT);
  return entry("", CURRENT_SESSION_START);
}

function foreignEntry(event: string): Entry {
  return event === "Stop" ? entry("", FOREIGN_SCRIPT, "acme-audit") : entry("*", FOREIGN_LOCAL_HTTP);
}

const SETTINGS_PATH = "/home/test/.claude/settings.json";
const bundle = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");

function loadClaude(files: Map<string, string>) {
  const host = {
    manifestJson: () => "null",
    homeDir: () => "/home/test",
    path: { equal: (a: string, b: string) => a === b },
    platform: () => "linux",
    unixNowMs: () => 0,
    envGet: () => null,
    shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
    expandHome: () => SETTINGS_PATH,
    fileExists: (p: string) => files.has(p),
    writeFile: (p: string, v: string) => { files.set(p, v); return true; },
    removeFile: (p: string) => files.delete(p),
    log: () => {},
    fs: {
      fileExists: (p: string) => files.has(p),
      readFile: (p: string) => files.get(p) ?? null,
      readJson: (p: string) => {
        const raw = files.get(p);
        if (raw === undefined) return null;
        try { return JSON.parse(raw); } catch { return null; }
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

function settingsWith(hooks: Record<string, Entry[]>): Map<string, string> {
  return new Map([[SETTINGS_PATH, JSON.stringify({ model: "opus", hooks })]]);
}

// ── current_unmarked_codeterm_hook_is_owned ────────────────────────────────
{
  console.log("current_unmarked_codeterm_hook_is_owned");
  const hooks: Record<string, Entry[]> = {};
  for (const event of EVENTS) hooks[event] = [currentUnmarked(event)];
  const files = settingsWith(hooks);
  const p = loadClaude(files);
  const result = p.installIntegration(7686, { consent: true, precompact_memory_export: true, repair: true });
  assert(result.ok === true, "install over the unmarked set succeeds");
  const written = JSON.parse(files.get(SETTINGS_PATH) as string);
  for (const event of EVENTS) {
    eq(written.hooks[event].length, 1, `${event}: the unmarked entry was replaced, not duplicated`);
    assert(
      String(written.hooks[event][0].hooks[0].command).indexOf(MARKER_HEADER) !== -1,
      `${event}: the replacement carries the durable marker`,
    );
  }
  eq(written.hooks.PermissionRequest.length, 1, "PermissionRequest keeps exactly one entry");
  eq(written.hooks.Stop.length, 1, "Stop keeps exactly one entry");
}

// ── legacy_v2_codeterm_hook_is_owned ───────────────────────────────────────
{
  console.log("legacy_v2_codeterm_hook_is_owned");
  const files = settingsWith({
    SessionStart: [
      entry("", LEGACY_V2_SESSION_START, "codeterm-hook-v2"),
      entry("", LEGACY_V2_SESSION_START),
    ],
    Stop: [entry("", LEGACY_V2_STOP), entry("", LEGACY_V2_STOP, "codeterm")],
  });
  const p = loadClaude(files);
  const before = p.checkIntegration();
  eq(before.integration_owned, true, "the exact pane fingerprint grants startup ownership");
  eq(before.hook_contract_current, false, "the pane contract is stale before reconciliation");
  assert(p.installIntegration(7686, { consent: true, repair: true }).ok === true, "install over v2 succeeds");
  const written = JSON.parse(files.get(SETTINGS_PATH) as string);
  eq(written.hooks.SessionStart.length, 1, "marked and _source-stripped v2 both collapse into one entry");
  eq(written.hooks.Stop.length, 1, "the pre-marker generation collapses too");
  assert(
    JSON.stringify(written.hooks).indexOf("CODETERM_PANE_ID") === -1,
    "no pane-era command survives the migration",
  );
}

{
  const files = settingsWith({ SessionStart: [entry("", FOREIGN_PANE_ROUTE)] });
  const p = loadClaude(files);
  const status = p.checkIntegration();
  eq(status.integration_owned, false, "a similar pane route does not imply CodeTerm ownership");
  p.installIntegration(7686, { consent: true, repair: true });
  const hooks = JSON.parse(files.get(SETTINGS_PATH) as string).hooks.SessionStart;
  eq(hooks[0], entry("", FOREIGN_PANE_ROUTE), "the foreign route is byte-identical after install");
}

// ── foreign_hook_is_not_owned ──────────────────────────────────────────────
{
  console.log("foreign_hook_is_not_owned");
  const hooks: Record<string, Entry[]> = {};
  for (const event of EVENTS) hooks[event] = [foreignEntry(event)];
  const files = settingsWith(hooks);
  const p = loadClaude(files);
  assert(p.installIntegration(7686, { consent: true, precompact_memory_export: true, repair: true }).ok === true,
    "install alongside foreign hooks succeeds");
  const written = JSON.parse(files.get(SETTINGS_PATH) as string);
  for (const event of EVENTS) {
    eq(written.hooks[event].length, 2, `${event}: the foreign entry survives beside ours`);
    eq(written.hooks[event][0], foreignEntry(event), `${event}: the foreign entry is byte-identical`);
  }
}

// ── reconcile_current_hooks_twice_keeps_one_entry_per_event ────────────────
{
  console.log("reconcile_current_hooks_twice_keeps_one_entry_per_event");
  const hooks: Record<string, Entry[]> = {};
  for (const event of EVENTS) hooks[event] = [foreignEntry(event), currentUnmarked(event)];
  hooks.SessionStart.push(entry("", LEGACY_V2_SESSION_START, "codeterm-hook-v2"));
  const files = settingsWith(hooks);
  const p = loadClaude(files);
  const options = { consent: true, precompact_memory_export: true, repair: true };
  assert(p.installIntegration(7686, options).ok === true, "first reconciliation succeeds");
  const first = files.get(SETTINGS_PATH) as string;
  assert(p.installIntegration(7686, options).ok === true, "second reconciliation succeeds");
  const second = files.get(SETTINGS_PATH) as string;
  eq(first, second, "a second reconciliation writes nothing new");

  const written = JSON.parse(second);
  eq(written.model, "opus", "unrelated settings untouched");
  for (const event of EVENTS) {
    eq(written.hooks[event].length, 2, `${event}: one foreign entry plus exactly one of ours`);
    const command = String(written.hooks[event][1].hooks[0].command);
    assert(command.indexOf(MARKER_HEADER) !== -1, `${event}: canonical entry carries the marker`);
    assert(
      command.indexOf("localhost") === -1 && command.indexOf("7685") === -1
      && command.indexOf("7686") === -1 && command.indexOf("CODETERM_API_PORT") === -1,
      `${event}: instance-agnostic, no port baked in`,
    );
    assert(command.indexOf("$CODETERM_API_TOKEN") !== -1 && command.indexOf("Bearer ct_") === -1,
      `${event}: the token is read from the environment, never embedded`);
  }
  const status = loadClaude(files).checkIntegration();
  eq(status.hooks_installed, true, "the reconciled set reports installed");
  eq(status.integration_owned, true, "the reconciled set reports marker ownership");
  eq(status.precompact_hook, undefined, "retired export has no status control");
  eq(status.hook_contract_current, true, "and reports its contract current, so nothing writes again");
}

{
  const files = settingsWith({ PreCompact: [foreignEntry("PreCompact")] });
  const status = loadClaude(files).checkIntegration();
  eq(status.integration_owned, false, "a foreign hook does not imply CodeTerm consent");
  eq(status.precompact_hook, undefined, "a foreign PreCompact hook does not imply CodeTerm export opt-in");
}

{
  const files = settingsWith({
    ...Object.fromEntries(EVENTS.map((event) => [event, [currentUnmarked(event)]])),
  });
  const status = loadClaude(files).checkIntegration();
  eq(status.integration_owned, false, "pre-marker compatibility hooks do not imply recorded consent");
  eq(status.precompact_hook, undefined, "pre-marker PreCompact is not treated as an explicit export choice");
}

{
  const files = new Map([[SETTINGS_PATH, "{"]]);
  const status = loadClaude(files).checkIntegration();
  eq(status.integration_state, "unreadable", "invalid settings are reported as unreadable");
}

{
  const devFiles = settingsWith({});
  const prodFiles = settingsWith({});
  loadClaude(devFiles).installIntegration(7686, { consent: true, precompact_memory_export: true, repair: true });
  loadClaude(prodFiles).installIntegration(7685, { consent: true, precompact_memory_export: true, repair: true });
  const dev = JSON.parse(devFiles.get(SETTINGS_PATH) as string);
  const prod = JSON.parse(prodFiles.get(SETTINGS_PATH) as string);
  eq(dev.hooks, prod.hooks, "dev and prod receive byte-identical hook commands");
  const written = JSON.stringify(dev.hooks);
  assert(written.indexOf("$CODETERM_API_URL") !== -1, "hooks route through the agent environment");
  assert(written.indexOf("$CODETERM_API_TOKEN") !== -1, "hooks read the token from the environment");
  assert(written.indexOf("$CODETERM_TAB_ID") !== -1, "hooks read the tab identity from the environment");
  assert(written.indexOf("localhost") === -1 && written.indexOf("7685") === -1
    && written.indexOf("7686") === -1 && written.indexOf("CODETERM_API_PORT") === -1,
  "hook commands contain no instance host or port");
}

{
  const foreign = foreignEntry("PreCompact");
  const owned = entry("", CURRENT_PRECOMPACT, "codeterm-hook-v3");
  const files = settingsWith({ PreCompact: [owned, foreign] });
  const provider = loadClaude(files);
  assert(provider.installIntegration(7686, {
    consent: true, precompact_memory_export: true, repair: true,
  }).ok === true, "retired export cleanup installs normal hooks");
  const written = JSON.parse(files.get(SETTINGS_PATH) as string);
  eq(written.hooks.PreCompact, [foreign], "retired export removes only the owned PreCompact hook");
  assert(written.hooks.SessionStart.length > 0 && written.hooks.Stop.length > 0,
    "retired export cleanup preserves session and completion hooks");
}

{
  const foreign = { type: "command", command: "foreign-shared-group", timeout: 1234 };
  const group = { matcher: "*", hooks: [...entry("", CURRENT_PRECOMPACT, "codeterm-hook-v3").hooks!, foreign] };
  for (const operation of ["install", "uninstall"]) {
    const files = settingsWith({ PreCompact: [group] });
    const provider = loadClaude(files);
    const result = operation === "install"
      ? provider.installIntegration(7686, { consent: true, repair: true })
      : provider.uninstallIntegration();
    assert(result.ok === true, operation + " accepts mixed retired hook group");
    const written = JSON.parse(files.get(SETTINGS_PATH) as string);
    eq(written.hooks.PreCompact, [{ matcher: "*", hooks: [foreign] }], operation + " preserves foreign sibling in retired hook group");
  }
}

{
  const files = settingsWith({ Stop: [foreignEntry("Stop")] });
  const provider = loadClaude(files);
  provider.installIntegration(7686, { consent: true, repair: true });
  const installed = JSON.parse(files.get(SETTINGS_PATH) as string);
  const before = files.get(SETTINGS_PATH);
  eq(provider.checkIntegration().hook_contract_current, true, "five-second hooks are current");
  for (const event of EVENTS) {
    const owned = installed.hooks[event].find((item: Entry) =>
      item.hooks.some((hook) => hook.command.includes(MARKER_HEADER)));
    eq(owned.hooks[0].timeout, 5, event + " deadline uses vendor seconds");
    owned.hooks[0].timeout = 5000;
  }
  files.set(SETTINGS_PATH, JSON.stringify(installed));
  eq(provider.checkIntegration().hook_contract_current, false, "millisecond-shaped timeouts require consented repair");
  provider.installIntegration(7685, { consent: true, repair: true });
  eq(files.get(SETTINGS_PATH), before, "repair is instance independent and preserves foreign hooks");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
