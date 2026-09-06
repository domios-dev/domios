/**
 * Codex Windows launch-profile sweep tests (codex profile lifecycle).
 * Run: npx tsx plugins/provider-codex/tests/profile-sweep.test.ts
 *
 * Proves BOTH directions the sweep must get right: a profile naming a tab id
 * that is not live gets removed, and a profile naming a tab id that IS live
 * is left untouched. A sweep that can remove a live tab's profile must never
 * ship — these tests exist to catch exactly that regression.
 */

import { codexProfileTabId, codexProfilesToSweep, sweepCodexProfiles, type SweepHost } from "../src/profile-sweep";

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

console.log("\n--- codexProfileTabId ---\n");
{
  eq(codexProfileTabId("codeterm-abc123.config.toml"), "abc123", "extracts the tab id from an owned profile name");
  eq(codexProfileTabId("codeterm-tab-with-dashes_and_underscores.config.toml"), "tab-with-dashes_and_underscores",
    "tab ids may contain dashes and underscores");
  eq(codexProfileTabId("config.toml"), null, "the base config file is never treated as an owned profile");
  eq(codexProfileTabId("codeterm-abc123.config.toml.codeterm-backup"), null,
    "a backup/sibling file with extra suffix is not matched");
  eq(codexProfileTabId("someone-elses-file.toml"), null, "a file CodeTerm didn't write is never matched");
}

console.log("\n--- codexProfilesToSweep: both directions ---\n");
{
  const files = [
    "codeterm-dead-tab.config.toml",
    "codeterm-live-tab.config.toml",
    "config.toml",
    "hooks.json",
  ];
  const live = new Set(["live-tab"]);
  const toRemove = codexProfilesToSweep(files, live);

  assert(toRemove.includes("codeterm-dead-tab.config.toml"), "a profile naming a NON-live tab id is swept");
  assert(!toRemove.includes("codeterm-live-tab.config.toml"), "a profile naming a LIVE tab id is preserved");
  assert(!toRemove.includes("config.toml"), "a file that isn't an owned profile is never swept");
  assert(!toRemove.includes("hooks.json"), "an unrelated file is never swept");
  eq(toRemove.length, 1, "exactly one file matched the sweep in this fixture");
}
{
  // All-live and all-dead extremes, so neither direction is a coincidence of a mixed set.
  const files = ["codeterm-a.config.toml", "codeterm-b.config.toml"];
  eq(codexProfilesToSweep(files, new Set(["a", "b"])), [], "every profile live -> nothing swept");
  eq(
    codexProfilesToSweep(files, new Set()),
    ["codeterm-a.config.toml", "codeterm-b.config.toml"],
    "no tab live -> every owned profile swept",
  );
}

console.log("\n--- sweepCodexProfiles: fs-level both directions ---\n");
{
  function makeHost(platform: string, entries: { name: string; isFile: boolean }[], removed: string[]): SweepHost {
    return {
      platform: () => platform,
      homeDir: () => "C:\\Users\\test",
      fs: {
        readDir: () => entries,
        removeFile: (path: string) => { removed.push(path); return true; },
      },
    };
  }

  const removed: string[] = [];
  const host = makeHost("windows", [
    { name: "codeterm-dead-tab.config.toml", isFile: true },
    { name: "codeterm-live-tab.config.toml", isFile: true },
    { name: "config.toml", isFile: true },
    { name: ".codex", isFile: false },
  ], removed);

  const result = sweepCodexProfiles(host, ["live-tab"]);
  eq(result.removed, ["codeterm-dead-tab.config.toml"], "sweep reports exactly the dead-tab profile as removed");
  eq(removed, ["C:\\Users\\test\\.codex\\codeterm-dead-tab.config.toml"],
    "host.fs.removeFile is invoked only for the dead-tab profile's real path");
  assert(!removed.some((p) => p.indexOf("live-tab") !== -1), "removeFile is never called for the live tab's profile");
}
{
  const removed: string[] = [];
  const host = makeHostNonWindows(removed);
  const result = sweepCodexProfiles(host, []);
  eq(result.removed, [], "non-Windows platforms never sweep — the profile file is a Windows-only artefact");
  eq(removed, [], "removeFile is never called on a non-Windows platform");
}
{
  // No homeDir -> conservative no-op, never a wrong deletion.
  const removed: string[] = [];
  const host: SweepHost = {
    platform: () => "windows",
    homeDir: () => null,
    fs: { readDir: () => { throw new Error("must never be reached"); }, removeFile: (p: string) => { removed.push(p); return true; } },
  };
  const result = sweepCodexProfiles(host, ["anything"]);
  eq(result.removed, [], "an unresolvable home dir leaves every file in place");
  eq(removed, [], "removeFile is never called when the home dir can't be resolved");
}

function makeHostNonWindows(removed: string[]): SweepHost {
  return {
    platform: () => "darwin",
    homeDir: () => "/Users/test",
    fs: {
      readDir: () => { throw new Error("must never be reached on non-Windows"); },
      removeFile: (path: string) => { removed.push(path); return true; },
    },
  };
}

console.log(`\n${passed} passed, ${failed} failed\n`);
if (failed > 0) process.exit(1);
