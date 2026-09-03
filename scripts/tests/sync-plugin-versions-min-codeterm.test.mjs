// codeterm-releases#62: the channel must serve an older app the newest
// manifest it can run, not the newest manifest that exists. That only works if
// minCodeterm actually tracks each system plugin's own version — a minCodeterm
// left frozen at its initial value is a no-op gate: it can never block an app
// older than the build it shipped with, so the channel always serves the
// absolute newest plugin build to every app regardless of version.
//
// This asserts the source of sync-plugin-versions.mjs directly rather than
// executing it: the script resolves every path relative to its own file
// location (not an injectable cwd), so running it for real would mutate this
// repository's actual plugin manifests — never acceptable from a test.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const SCRIPT = path.join(ROOT, "scripts", "sync-plugin-versions.mjs");

function source() {
  return readFileSync(SCRIPT, "utf8");
}

// Isolates the `for (const id of providers) { ... }` loop body that syncs each
// provider's own plugins/provider-<id>/plugin.json.
function providerPluginJsonBlock(text) {
  const start = text.indexOf('for (const id of providers) {');
  assert.notEqual(start, -1, "sync-plugin-versions.mjs lost its provider sync loop");
  const pluginJsonCall = text.indexOf('"plugin.json"', start);
  assert.notEqual(pluginJsonCall, -1, "provider loop no longer touches plugin.json");
  const blockEnd = text.indexOf('updateJson(', pluginJsonCall + 1);
  return text.slice(pluginJsonCall, blockEnd === -1 ? undefined : blockEnd);
}

// Isolates the channel.json mutation block.
function channelJsonBlock(text) {
  const start = text.indexOf('"channel.json"');
  assert.notEqual(start, -1, "sync-plugin-versions.mjs no longer syncs plugins/channel.json");
  return text.slice(start);
}

test("provider plugin.json sync keeps minCodeterm locked to the plugin's own version", () => {
  const block = providerPluginJsonBlock(source());
  assert.match(
    block,
    /json\.minCodeterm\s*=\s*version/,
    "provider plugin.json sync must set minCodeterm = version alongside json.version, " +
      "or an app older than this build silently keeps taking the newest plugin build",
  );
});

test("channel.json sync keeps minCodeterm locked to each provider's own version", () => {
  const block = channelJsonBlock(source());
  assert.match(
    block,
    /plugin\.minCodeterm\s*=\s*version/,
    "channel.json provider entries must set minCodeterm = version alongside plugin.version",
  );
});
