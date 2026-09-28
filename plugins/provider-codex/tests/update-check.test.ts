import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { checkCodexUpdate, updateAvailable } from "../src/update";

const here = path.dirname(fileURLToPath(import.meta.url));
type CodexPlugin = { checkForUpdate?: () => { latestVersion: string } | null };
let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}`); }
}

function fixture(name: string): Record<string, string> {
  return JSON.parse(fs.readFileSync(path.join(here, "fixtures", "update-check", name), "utf8")) as Record<string, string>;
}

function loadPlugin(installed: string, latest: string) {
  const pluginPath = path.join(here, "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const host = {
    exec: () => JSON.stringify({ code: 0, stdout: `codex ${installed}` }),
    fetch: () => JSON.stringify({ status: 200, body: JSON.stringify({ version: latest }) }),
  };
  const context = { host, console, module: { exports: {} as { default?: CodexPlugin } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const current = fixture("current.json");
const available = fixture("available.json");
assert(!updateAvailable(current.currentVersion, current.latestVersion), "current versions do not offer an update");
assert(updateAvailable(available.currentVersion, available.latestVersion), "newer versions are available");
assert(loadPlugin(available.currentVersion, available.latestVersion)?.checkForUpdate?.()?.latestVersion === available.latestVersion,
  "the provider bridge offers the checked newer version");
assert(loadPlugin(current.currentVersion, current.latestVersion)?.checkForUpdate?.() === null,
  "the provider bridge returns no update for the current version");
assert(checkCodexUpdate({
  exec: () => JSON.stringify({ code: 1, stdout: "", stderr: "not installed" }),
  fetch: () => JSON.stringify({ status: 200, body: JSON.stringify({ version: "99.0.0" }) }),
}) === null, "an unavailable CLI fails closed");

console.log(`\nupdate-check: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exitCode = 1;
