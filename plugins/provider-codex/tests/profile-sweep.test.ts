import assert from "node:assert/strict";
import {
  codexProfileInstanceId,
  codexProfileTabId,
  manageCodexProfiles,
  releaseCodexProfile,
  sweepCodexProfiles,
  type ProfileSweepHost,
} from "../src/profile-sweep";

function eq(actual: unknown, expected: unknown, message: string): void {
  assert.deepEqual(actual, expected, message);
}

function makeHost(files: Record<string, string>, unreadableDirectory = false) {
  const removed: string[] = [];
  const host: ProfileSweepHost = {
    platform: () => "windows",
    homeDir: () => "C:\\Users\\test",
    fs: {
      readDir: () => {
        if (unreadableDirectory) throw new Error("denied");
        return Object.keys(files).map((name) => ({ name, isFile: true }));
      },
      readFile: (path) => files[path.slice(path.lastIndexOf("\\") + 1)] ?? null,
      removeFile: (path) => {
        const name = path.slice(path.lastIndexOf("\\") + 1);
        removed.push(name);
        delete files[name];
        return true;
      },
    },
  };
  return { host, files, removed };
}

console.log("\n--- profile marker and name parsing ---\n");
eq(codexProfileTabId("codeterm-tab-with-dashes_and_underscores.config.toml"), "tab-with-dashes_and_underscores",
  "extracts only a CodeTerm profile tab id");
eq(codexProfileTabId("config.toml"), null, "does not match the base Codex config");
eq(codexProfileTabId("codeterm-tab.config.toml.codeterm-backup"), null, "does not match backup names");
eq(codexProfileInstanceId("\uFEFF# codeterm-instance: instance-dev\r\ndeveloper_instructions = \"x\""), "instance-dev",
  "reads the instance marker from the first TOML line");
eq(codexProfileInstanceId("developer_instructions = \"x\"\n# codeterm-instance: instance-dev"), null,
  "does not accept a marker after the first line");

console.log("\n--- automatic sweep only removes this instance's dead profiles ---\n");
{
  const { host, files, removed } = makeHost({
    "codeterm-dead-tab.config.toml": "# codeterm-instance: instance-dev\ndeveloper_instructions = \"dead\"",
    "codeterm-live-tab.config.toml": "# codeterm-instance: instance-dev\ndeveloper_instructions = \"live\"",
    "codeterm-foreign-tab.config.toml": "# codeterm-instance: instance-prod\ndeveloper_instructions = \"foreign\"",
    "codeterm-legacy-tab.config.toml": "developer_instructions = \"legacy\"",
    "foreign.config.toml": "# codeterm-instance: instance-dev",
  });
  const report = sweepCodexProfiles(host, ["live-tab"], "instance-dev");
  eq(report.dead, ["codeterm-dead-tab.config.toml"], "reports only a dead profile owned by this instance");
  eq(report.removed, ["codeterm-dead-tab.config.toml"], "removes only the owned dead profile");
  eq(removed, ["codeterm-dead-tab.config.toml"], "never removes another instance or unmarked profile");
  assert.ok(files["codeterm-live-tab.config.toml"], "keeps this instance's live tab profile");
  assert.ok(files["codeterm-foreign-tab.config.toml"], "keeps another instance's unknown tab profile");
  assert.ok(files["codeterm-legacy-tab.config.toml"], "keeps the unmarked legacy profile automatically");
}

console.log("\n--- unreadable directories fail closed ---\n");
{
  const { host, removed } = makeHost({
    "codeterm-dead-tab.config.toml": "# codeterm-instance: instance-dev",
  }, true);
  const report = sweepCodexProfiles(host, [], "instance-dev");
  eq(report.removed, [], "reports no removals after a directory read failure");
  eq(removed, [], "keeps files after a directory read failure");
}

console.log("\n--- owner command dry run and legacy opt-in ---\n");
{
  const { host, files, removed } = makeHost({
    "codeterm-dead-tab.config.toml": "# codeterm-instance: instance-dev",
    "codeterm-legacy-tab.config.toml": "developer_instructions = \"legacy\"",
    "codeterm-foreign-tab.config.toml": "# codeterm-instance: instance-prod",
  });
  const liveTabIds = ["legacy-tab"];
  const preview = manageCodexProfiles(host, liveTabIds, "instance-dev", false, false);
  eq(preview.dead, ["codeterm-dead-tab.config.toml"], "lists owned dead profiles by default");
  eq(preview.legacy, ["codeterm-legacy-tab.config.toml"], "lists unmarked profiles for owner review");
  eq(preview.removed, [], "dry run does not delete files");
  eq(removed, [], "dry run leaves profiles intact");
  const confirmed = manageCodexProfiles(host, liveTabIds, "instance-dev", true, false);
  eq(confirmed.removed, ["codeterm-dead-tab.config.toml"], "confirm removes owned dead profiles only");
  assert.ok(files["codeterm-legacy-tab.config.toml"], "legacy files require --include-legacy");
  const legacyConfirmed = manageCodexProfiles(host, liveTabIds, "instance-dev", true, true);
  eq(legacyConfirmed.removed, ["codeterm-legacy-tab.config.toml"], "includeLegacy plus confirm deletes unmarked profiles regardless of tab liveness");
}

console.log("\n--- close releases only the profile marked by this instance ---\n");
{
  const { host, removed } = makeHost({
    "codeterm-tab.config.toml": "# codeterm-instance: instance-dev",
    "codeterm-foreign.config.toml": "# codeterm-instance: instance-prod",
  });
  eq(releaseCodexProfile(host, "tab", "instance-dev"), true, "removes the tab's own profile");
  eq(releaseCodexProfile(host, "foreign", "instance-dev"), false, "keeps a profile marked by another instance");
  eq(removed, ["codeterm-tab.config.toml"], "only the same-instance file was removed");
}

console.log("\nPASS instance-scoped Codex profile cleanup\n");
