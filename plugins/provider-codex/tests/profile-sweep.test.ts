import assert from "node:assert/strict";
import {
  codexProfileInstanceId,
  codexProfileTabId,
  carryLiveCodexProjectTrust,
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
      fileExists: (path) => files[path.slice(path.lastIndexOf("\\") + 1)] !== undefined,
      isSymlink: () => false,
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
      writeFileAtomic: (path, contents) => {
        files[path.slice(path.lastIndexOf("\\") + 1)] = contents;
        return true;
      },
      lockFile: () => "lock",
      unlockFile: () => true,
    },
  };
  return { host, files, removed };
}

console.log("\n--- profile marker and name parsing ---\n");
eq(codexProfileTabId("codeterm-tab-with-dashes_and_underscores.config.toml"), "tab-with-dashes_and_underscores",
  "extracts only a CodeTerm profile tab id");
eq(codexProfileTabId("codeterm-tab-preserved.config.toml"), "tab",
  "extracts the tab id from its bounded preserved profile");
eq(codexProfileTabId("codeterm-tab-retry-42-1700000000000.config.toml"), "tab",
  "extracts the tab id from a retry profile");
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
    "codeterm-dead-tab-preserved.config.toml": "# codeterm-instance: instance-dev\ndeveloper_instructions = \"preserved\"",
    "codeterm-dead-tab-retry-42-1700000000000.config.toml": "# codeterm-instance: instance-dev\ndeveloper_instructions = \"retry\"",
    "codeterm-live-tab.config.toml": "# codeterm-instance: instance-dev\ndeveloper_instructions = \"live\"",
    "codeterm-foreign-tab.config.toml": "# codeterm-instance: instance-prod\ndeveloper_instructions = \"foreign\"",
    "codeterm-legacy-tab.config.toml": "developer_instructions = \"legacy\"",
    "foreign.config.toml": "# codeterm-instance: instance-dev",
  });
  const report = sweepCodexProfiles(host, ["live-tab"], "instance-dev");
  eq(report.dead, [
    "codeterm-dead-tab.config.toml",
    "codeterm-dead-tab-preserved.config.toml",
    "codeterm-dead-tab-retry-42-1700000000000.config.toml",
  ], "reports all dead profiles owned by this instance");
  eq(report.removed, report.dead, "removes owned standard, preserved, and retry profiles");
  eq(removed, report.dead, "never removes another instance or unmarked profile");
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

console.log("\n--- profile release carries project trust before removal ---\n");
{
  const config = [
    "[hooks.state.'C:\\Users\\test\\.codex\\hooks.json:session_start:0:0']",
    'trusted_hash = "sha256:unchanged"',
    "",
  ].join("\r\n");
  const { host, files } = makeHost({
    "codeterm-tab.config.toml": [
      "# codeterm-instance: instance-dev",
      'developer_instructions = "brief"',
      "",
      "[projects.'d:\\developer\\trust-probe']",
      'trust_level = "trusted"',
      "",
    ].join("\r\n"),
    "config.toml": config,
  });
  assert.equal(releaseCodexProfile(host, "tab", "instance-dev"), true);
  assert.equal(files["codeterm-tab.config.toml"], undefined, "profile is removed after trust carry-over");
  assert.ok(files["config.toml"].includes("[projects.'d:\\developer\\trust-probe']"));
  assert.ok(files["config.toml"].includes('trusted_hash = "sha256:unchanged"'));
}

console.log("\n--- live trust answers reach shared config before profile release ---\n");
{
  const config = "[hooks.state]\r\ntrusted_hash = \"sha256:unchanged\"\r\n";
  const { host, files } = makeHost({
    "codeterm-live-tab.config.toml": [
      "# codeterm-instance: instance-dev",
      "",
      "[projects.'C:\\Users\\test\\TeamProject']",
      'trust_level = "trusted"',
      "",
    ].join("\r\n"),
    "codeterm-foreign-tab.config.toml": [
      "# codeterm-instance: instance-prod",
      "[projects.'C:\\Foreign']",
      'trust_level = "trusted"',
      "",
    ].join("\n"),
    "codeterm-legacy-tab.config.toml": [
      "[projects.'C:\\Legacy']",
      'trust_level = "trusted"',
      "",
    ].join("\n"),
    "config.toml": config,
  });
  assert.equal(carryLiveCodexProjectTrust(host), true, "live profiles can persist trust while their tabs remain open");
  assert.ok(files["config.toml"].includes("[projects.'C:\\Users\\test\\TeamProject']"),
    "the live tab trust is immediately available to sibling and later Codex launches");
  assert.ok(!files["config.toml"].includes("C:\\Foreign"), "does not collect another instance's profile trust");
  assert.ok(!files["config.toml"].includes("C:\\Legacy"), "does not collect unmarked profile trust");
  assert.ok(files["config.toml"].startsWith(config), "hooks and trusted_hash bytes remain unchanged");
}

console.log("\n--- dead tab profile carries trust before startup sweep removes it ---\n");
{
  const config = [
    "[hooks.state.'C:\\Users\\test\\.codex\\hooks.json:session_start:0:0']",
    'trusted_hash = "sha256:unchanged"',
    "",
  ].join("\r\n");
  const profileName = "codeterm-tab-lost-without-release.config.toml";
  const { host, files, removed } = makeHost({
    [profileName]: [
      "# codeterm-instance: instance-dev",
      "",
      "[projects.'C:\\Users\\test\\modeprobe-scratch\\codex']",
      'trust_level = "trusted"',
      "",
    ].join("\r\n"),
    "config.toml": config,
  });

  const report = sweepCodexProfiles(host, ["new-tab"], "instance-dev");

  eq(report.dead, [profileName], "finds a same-instance profile whose tab disappeared");
  eq(report.removed, [profileName], "removes the dead profile only after trust carry-over");
  eq(removed, [profileName], "cleanup removes the dead profile after persistence");
  assert.equal(files[profileName], undefined, "the lost tab profile is no longer orphaned");
  assert.ok(files["config.toml"].startsWith(config),
    "the existing hooks and trusted_hash bytes are preserved");
  assert.ok(files["config.toml"].includes("[projects.'C:\\Users\\test\\modeprobe-scratch\\codex']"),
    "the trusted project reaches the shared config for the next spawn");
  assert.ok(files["config.toml"].includes('trusted_hash = "sha256:unchanged"'),
    "carry-over preserves the hook trust hash byte content");
}

console.log("\n--- retry and preserved profiles carry trust before removal ---\n");
{
  const config = [
    "[hooks.state.'C:\\Users\\test\\.codex\\hooks.json:session_start:0:0']",
    'trusted_hash = "sha256:unchanged"',
    "",
  ].join("\r\n");
  const { host, files } = makeHost({
    "codeterm-tab-retry-42-1700000000000.config.toml": [
      "# codeterm-instance: instance-dev",
      "",
      "[projects.'C:\\RetryProject']",
      'trust_level = "trusted"',
      "",
    ].join("\r\n"),
    "codeterm-tab-preserved.config.toml": [
      "# codeterm-instance: instance-dev",
      "",
      "[projects.'C:\\PreservedProject']",
      'trust_level = "trusted"',
      "",
    ].join("\r\n"),
    "config.toml": config,
  });
  assert.equal(releaseCodexProfile(host, "tab", "instance-dev"), true);
  assert.equal(files["codeterm-tab-retry-42-1700000000000.config.toml"], undefined);
  assert.equal(files["codeterm-tab-preserved.config.toml"], undefined);
  assert.ok(files["config.toml"].includes("[projects.'C:\\RetryProject']"));
  assert.ok(files["config.toml"].includes("[projects.'C:\\PreservedProject']"));
  assert.ok(files["config.toml"].includes('trusted_hash = "sha256:unchanged"'));
}

console.log("\n--- profile remains when the shared config lock is busy ---\n");
{
  const { host, files, removed } = makeHost({
    "codeterm-tab.config.toml": [
      "# codeterm-instance: instance-dev",
      "",
      "[projects.'C:\\Project']",
      'trust_level = "trusted"',
      "",
    ].join("\r\n"),
    "config.toml": "",
  });
  host.fs.lockFile = () => null;
  assert.equal(releaseCodexProfile(host, "tab", "instance-dev"), false,
    "a busy lock refuses profile release so a later close or sweep can retry");
  assert.ok(files["codeterm-tab.config.toml"], "the trust answer remains in the active profile");
  eq(removed, [], "lock contention does not delete the profile");
}

console.log("\nPASS instance-scoped Codex profile cleanup\n");
