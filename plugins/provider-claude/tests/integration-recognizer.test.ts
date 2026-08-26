/**
 * The integration recognizer must not key on a field the host app can strip.
 * Run: npx tsx plugins/provider-claude/tests/integration-recognizer.test.ts
 *
 * Claude Code rewrites ~/.claude/settings.json whenever any of its own settings
 * change, and its serializer drops `_source` because that field is not part of
 * its schema. The hooks keep working — the commands are untouched and still
 * carry the binding nonce — but a recognizer that required `_source` reported
 * them missing, so "registers no sessions" fired falsely and re-fired after
 * every settings change. Identity is the command signature; `_source` is a
 * dedup hint.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import Module from "node:module";

const here = path.dirname(fileURLToPath(import.meta.url));
const bundle = path.resolve(here, "../plugin.js");

const HOOK_COMMAND =
  'if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then ' +
  'curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start" ' +
  '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"; fi';

const EVENTS = ["Notification", "PermissionRequest", "Stop", "SessionStart"];

function settings(withSource: boolean): unknown {
  const hook: Record<string, unknown> = { type: "command", command: HOOK_COMMAND };
  if (withSource) hook._source = "codeterm-hook-v3";
  return {
    model: "opus",
    theme: "dark",
    hooks: Object.fromEntries(EVENTS.map((evt) => [evt, [{ hooks: [hook] }]])),
  };
}

function loadPlugin(current: unknown) {
  const g = globalThis as Record<string, unknown>;
  g.host = {
    expandHome: (p: string) => p,
    fileExists: () => true,
    fs: { readJson: () => current, exists: () => true },
    log: () => {},
  };
  const asCjs = path.join(path.dirname(bundle), ".integration-recognizer.cjs");
  fs.copyFileSync(bundle, asCjs);
  const require_ = Module.createRequire(import.meta.url);
  delete require_.cache?.[asCjs];
  try {
    const mod = require_(asCjs) as { default?: unknown };
    return (mod.default ?? mod) as { checkIntegration(): Record<string, unknown> };
  } finally {
    fs.rmSync(asCjs, { force: true });
  }
}

let failures = 0;
function check(label: string, actual: unknown, expected: unknown) {
  const ok = actual === expected;
  if (!ok) failures++;
  console.log(`${ok ? "  ✓" : "  ✗"} ${label}: ${String(actual)}${ok ? "" : ` (expected ${String(expected)})`}`);
}

for (const withSource of [true, false]) {
  const label = withSource ? "_source present" : "_source stripped by the host";
  const status = loadPlugin(settings(withSource)).checkIntegration();
  check(`${label} — session_start_hook`, status.session_start_hook, true);
  check(`${label} — hooks_installed`, status.hooks_installed, true);
}

console.log(failures === 0 ? "✓ integration recognizer: 4/4" : `✗ integration recognizer: ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
