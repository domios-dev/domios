import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const home = "/tmp/codex-trust-status";
const hooksPath = `${home}/.codex/hooks.json`;
const configPath = `${home}/.codex/config.toml`;
const sessionKey = `${hooksPath}:session_start:0:0`;
const stopKey = `${hooksPath}:stop:0:0`;
const command = (event: string) =>
  `curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/${event}" `
    + '-H "Authorization: Bearer ${CODETERM_API_TOKEN}" '
    + '-H "X-CodeTerm-Hook: codeterm-codex-session-binding-v1" '
    + '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"';

function fixtureFiles() {
  return new Map<string, string>([
    [hooksPath, JSON.stringify({
      hooks: {
        SessionStart: [{ hooks: [{ command: command("session_start"), timeout: 60 }] }],
        Stop: [{ hooks: [{ command: command("stop"), timeout: 60 }] }],
      },
    })],
    [configPath, [
      `[hooks.state.'${sessionKey}']`,
      'trusted_hash = "sha256:recorded-session"',
      `[hooks.state.'${stopKey}']`,
      'trusted_hash = "sha256:recorded-stop"',
      "",
    ].join("\n")],
  ]);
}

function load(files: Map<string, string>, exec: () => string) {
  let execCalls = 0;
  const runExec = Object.assign(
    () => { execCalls++; return exec(); },
    { async: (_opts: unknown, then: (result: unknown) => unknown) => { execCalls++; return then(exec()); } },
  );
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const host = {
    manifestJson: () => "null",
    homeDir: () => home,
    platform: () => "linux",
    unixNowMs: () => 1000,
    envGet: () => null,
    log: () => {},
    path: { equal: (left: string, right: string) => left === right },
    shell: { quoteFor: (value: unknown) => String(value), quote: (value: unknown) => String(value) },
    exec: runExec,
    fs: {
      fileExists: (file: string) => files.has(file),
      readFile: (file: string) => files.get(file) ?? null,
      readJson: (file: string) => {
        try { return JSON.parse(files.get(file) || "{}"); } catch { return null; }
      },
      writeFile: () => { throw new Error("status and startup checks are read-only"); },
      writeJson: () => { throw new Error("status and startup checks are read-only"); },
      removeFile: () => true,
    },
  };
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(fs.readFileSync(pluginPath, "utf8"), { filename: pluginPath }).runInNewContext(context);
  return { plugin: context.module.exports.default, calls: () => execCalls };
}

function appServerResult(sessionHash: string, stopHash: string, trustStatus = "untrusted"): string {
  const listings = [
    { key: sessionKey, command: command("session_start"), currentHash: sessionHash, trustStatus },
    { key: stopKey, command: command("stop"), currentHash: stopHash, trustStatus },
  ];
  return JSON.stringify({
    code: 0,
    stdout: [
      JSON.stringify({ id: 1, result: {} }),
      JSON.stringify({ id: 2, result: { data: [{ hooks: listings }] } }),
    ].join("\n"),
    stderr: "",
  });
}

{
  const files = fixtureFiles();
  const { plugin, calls } = load(files, () => appServerResult("sha256:current-session", "sha256:current-stop"));
  const initial = plugin.checkIntegration();
  assert.equal(calls(), 0, "status polling never starts codex app-server");
  assert.equal(initial.hook_trust_state, "unknown", "a recorded hash is not trusted without Codex confirmation");
  assert.equal(initial.hook_trust_verification_source, "unverified", "status distinguishes an unverified hash from a cached verdict");
  assert.equal(initial.hook_trust_verified_current, false, "unknown trust is not reported as current");

  const repair = plugin.ensureIntegrationOnStartup(7686);
  assert.equal(repair.actionable, true, "background reconciliation finds a current-content hash mismatch");
  const stale = plugin.checkIntegration();
  assert.equal(stale.hook_trust_state, "stale", "the cached Codex comparison reports stale trust");
  assert.equal(stale.integration_state, "untrusted_hash", "stale trust enters the existing consented repair path");
  assert.equal(stale.hook_trust_verification_source, "cached", "status identifies the cached app-server hash comparison");
  assert.equal(stale.hook_trust_verified_current, false, "a cached mismatch is not current trust");
  assert.equal(stale.proposed_changes.includes("hook trust"), true,
    "a cached mismatch still proposes hook trust verification");
  assert.equal(calls(), 1, "only the explicit background refresh used app-server");
  assert.equal(files.get(configPath)?.includes('trusted_hash = "sha256:recorded-session"'), true,
    "status and refresh do not write hook trust hashes");
}

{
  const files = fixtureFiles();
  const { plugin } = load(files, () => appServerResult("sha256:recorded-session", "sha256:recorded-stop", "trusted"));
  plugin.ensureIntegrationOnStartup(7686);
  const status = plugin.checkIntegration();
  assert.equal(status.hook_trust_state, "trusted", "Codex confirmed the current hook hashes");
  assert.equal(status.hook_trust_verification_source, "hash_verified", "status says the recorded trust hash matches Codex currentHash");
  assert.equal(status.hook_trust_verified_current, true, "trusted current hashes are explicit");
  assert.equal(status.proposed_changes.includes("hook trust"), false,
    "a current hash verdict removes hook trust from proposed changes");
}

{
  const files = fixtureFiles();
  let unavailable = false;
  const { plugin } = load(files, () => unavailable
    ? JSON.stringify({ code: 1, stdout: "", stderr: "codex unavailable" })
    : appServerResult("sha256:recorded-session", "sha256:recorded-stop", "trusted"));
  plugin.ensureIntegrationOnStartup(7686);
  files.set(configPath, files.get(configPath)!.replace("recorded-session", "updated-session"));
  unavailable = true;
  assert.equal(plugin.ensureIntegrationOnStartup(7686).actionable, true,
    "an unavailable app-server surfaces an unverified hook hash");
  assert.equal(plugin.checkIntegration().hook_trust_state, "unknown",
    "a changed config fingerprint is not treated as trusted without Codex confirmation");
}

{
  const files = fixtureFiles();
  let unavailable = false;
  const { plugin } = load(files, () => unavailable
    ? JSON.stringify({ code: 1, stdout: "", stderr: "codex unavailable" })
    : appServerResult("sha256:stale-session", "sha256:stale-stop"));
  plugin.ensureIntegrationOnStartup(7686);
  assert.equal(plugin.checkIntegration().hook_trust_state, "stale", "a proven mismatch is cached as stale");
  files.set(configPath, files.get(configPath)!.replace("recorded-session", "changed-session"));
  unavailable = true;
  plugin.ensureIntegrationOnStartup(7686);
  assert.equal(plugin.checkIntegration().hook_trust_state, "unknown",
    "a changed config fingerprint remains unknown until Codex proves it stale or trusted");
}

{
  const files = fixtureFiles();
  let hookContentChanged = false;
  const { plugin, calls } = load(files, () => appServerResult(
    hookContentChanged ? "sha256:changed-session" : "sha256:recorded-session",
    "sha256:recorded-stop",
    "trusted",
  ));
  plugin.ensureIntegrationOnStartup(7686);
  assert.equal(plugin.checkIntegration().hook_trust_state, "trusted", "a successful refresh caches its verdict");
  const hooks = JSON.parse(files.get(hooksPath)!);
  hooks.hooks.SessionStart[0].hooks[0].command += " --data-binary @-";
  files.set(hooksPath, JSON.stringify(hooks));
  hookContentChanged = true;
  assert.equal(plugin.checkIntegration().hook_trust_state, "unknown",
    "changed hook content invalidates the cached verdict until Codex compares hashes");
  plugin.ensureIntegrationOnStartup(7686);
  assert.equal(plugin.checkIntegration().hook_trust_state, "stale",
    "background refresh replaces the verdict after hook content changes");
  assert.equal(calls(), 2, "only explicit refreshes start codex app-server");
}

console.log("PASS Codex integration trust status stays off the hot path and preserves unknown verdicts");
