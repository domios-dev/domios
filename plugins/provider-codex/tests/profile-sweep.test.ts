import assert from "node:assert/strict";
import {
  codexProfileInstanceId,
  classifyCodexProfile,
  codexProfileTabId,
  carryCodexProjectTrustForTab,
  codexDirectoryTrustState,
  isCodexDirectoryTrusted,
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
    "codeterm-deadbeef.config.toml": "developer_instructions = \"legacy\"",
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
  assert.ok(files["codeterm-deadbeef.config.toml"], "keeps the unmarked legacy profile automatically");
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

console.log("\n--- launch trust state follows Codex trust-key precedence ---\n");
{
  const cwd = "C:\\ws\\repo\\lane";
  const projectRoot = "C:\\ws\\repo";
  const gitRoot = "C:\\ws";
  const configPath = "C:\\Users\\test\\.codex\\config.toml";
  const makeTrustHost = (config: string | null, symlinks: string[] = []) => {
    const host: ProfileSweepHost = {
      platform: () => "windows",
      homeDir: () => "C:\\Users\\test",
      fs: {
        fileExists: (path) => path === configPath || path === `${projectRoot}\\.codeterm-root` || path === `${gitRoot}\\.git`,
        isSymlink: (path) => symlinks.includes(path) ? true : false,
        readDir: () => [],
        readFile: (path) => path === configPath ? config : null,
        removeFile: () => false,
        writeFileAtomic: () => false,
        lockFile: () => null,
        unlockFile: () => false,
      },
    };
    return host;
  };
  const entries = (...values: Array<[string, "trusted" | "untrusted"]>) => values
    .map(([path, trust]) => `[projects.'${path}']\ntrust_level = "${trust}"`)
    .join("\n");

  eq(codexDirectoryTrustState(makeTrustHost(entries([cwd, "trusted"], [projectRoot, "untrusted"])), cwd),
    "trusted", "cwd exact key takes precedence over project root");
  eq(codexDirectoryTrustState(makeTrustHost(`project_root_markers = [\".codeterm-root\"]\n${entries([projectRoot, "untrusted"], [gitRoot, "trusted"])}`), cwd),
    "untrusted", "project-root key takes precedence over git root");
  eq(codexDirectoryTrustState(makeTrustHost(entries([gitRoot, "trusted"])), cwd),
    "trusted", "git-root key applies when nearer roots have no entry");
  eq(codexDirectoryTrustState(makeTrustHost(""), cwd),
    "untrusted", "no configured key means Codex's default untrusted decision");
  eq(codexDirectoryTrustState(makeTrustHost(null), cwd),
    "unknown", "an unreadable existing config cannot predict the prompt");
  eq(codexDirectoryTrustState(makeTrustHost("", [`${projectRoot}\\.git`]), cwd),
    "unknown", "a symlinked trust root cannot predict the prompt");
}

{
  const repo = "C:\\Users\\test\\repo";
  const lane = "C:\\Users\\test\\repo-worktrees\\lane-1";
  const configPath = "C:\\Users\\test\\.codex\\config.toml";
  const config = `[projects.'${repo}']\ntrust_level = "trusted"`;
  const laneGit = `${lane}\\.git`.toLowerCase();
  const host: ProfileSweepHost = {
    platform: () => "windows",
    homeDir: () => "C:\\Users\\test",
    fs: {
      fileExists: (path) => path === configPath || path.toLowerCase() === laneGit,
      isSymlink: () => false,
      readDir: () => [],
      readFile: (path) => path === configPath
        ? config
        : path.toLowerCase() === laneGit ? "gitdir: C:/Users/test/repo/.git/worktrees/lane-1\n" : null,
      removeFile: () => false,
      writeFileAtomic: () => false,
      lockFile: () => null,
      unlockFile: () => false,
    },
  };
  eq(codexDirectoryTrustState(host, lane), "trusted",
    "a linked worktree is trusted through its main repository, as Codex resolves it");
}

console.log("\n--- owner command dry run and legacy opt-in ---\n");
{
  const { host, files, removed } = makeHost({
    "codeterm-dead-tab.config.toml": "# codeterm-instance: instance-dev",
    "codeterm-deadbeef.config.toml": "developer_instructions = \"legacy\"",
    "codeterm-1234abcd.config.toml": "developer_instructions = \"live\"",
    "codeterm-foreign-tab.config.toml": "# codeterm-instance: instance-prod",
  });
  const liveTabIds = ["1234abcd"];
  const preview = manageCodexProfiles(host, liveTabIds, "instance-dev", false, false);
  eq(preview.dead, ["codeterm-dead-tab.config.toml"], "lists owned dead profiles by default");
  eq(preview.legacy, ["codeterm-deadbeef.config.toml"], "lists only exact-pattern dead legacy profiles");
  eq(preview.removed, [], "dry run does not delete files");
  eq(removed, [], "dry run leaves profiles intact");
  const confirmed = manageCodexProfiles(host, liveTabIds, "instance-dev", true, false);
  eq(confirmed.removed, ["codeterm-dead-tab.config.toml"], "confirm removes owned dead profiles only");
  assert.ok(files["codeterm-deadbeef.config.toml"], "legacy files require --include-legacy");
  const legacyConfirmed = manageCodexProfiles(host, liveTabIds, "instance-dev", true, true);
  eq(legacyConfirmed.removed, ["codeterm-deadbeef.config.toml"], "includeLegacy plus confirm deletes only dead exact-pattern legacy profiles");
  assert.ok(files["codeterm-1234abcd.config.toml"], "live legacy profiles survive explicit legacy cleanup");
}

console.log("\n--- pure cleanup policy ---\n");
{
  const own = "# codeterm-instance: owner\n";
  const foreign = "# codeterm-instance: foreign\n";
  const cases: [string, string, string[], string[], "dead" | "legacy" | null][] = [
    ["codeterm-deadbeef.config.toml", own, ["deadbeef"], [], null],
    ["codeterm-deadbeef.config.toml", own, [], [], "dead"],
    ["codeterm-deadbeef-retry-42-639263077363670423.config.toml", own, [], [], "dead"],
    ["codeterm-deadbeef-retry-42-639263077363670423.config.toml", own, ["deadbeef"], [], null],
    ["codeterm-deadbeef.config.toml", foreign, [], [], null],
    ["codeterm-deadbeef.config.toml", foreign, ["deadbeef"], ["foreign"], "dead"],
    ["codeterm-deadbeef.config.toml", "", ["deadbeef"], [], null],
    ["codeterm-deadbeef.config.toml", "", [], [], "legacy"],
    ["codeterm-deadbeef-preserved-42-123.config.toml", "", [], [], "legacy"],
    ["codeterm-deadbeef-retry-42-123.config.toml", "", ["deadbeef"], [], null],
    ["codeterm-user.config.toml", "", [], [], null],
    ["codeterm-deadbeef-custom.config.toml", "", [], [], null],
    ["codeterm-deadbeef-retry-x-123.config.toml", "", [], [], null],
    ["codeterm-ABCDEF12.config.toml", "", [], [], null],
    ["config.toml", own, [], [], null],
    ["user.config.toml", "", [], [], null],
    ["codeterm-deadbeef.config.toml", "# codeterm-instance: broken marker", [], [], null],
    ["codeterm-deadbeef.config.toml", "x\n# codeterm-instance: foreign", [], [], null],
  ];
  for (const [name, contents, live, gone, expected] of cases) {
    eq(classifyCodexProfile(name, contents, new Set(live), "owner", new Set(gone)), expected, name);
  }
}

console.log("\n--- foreign and legacy cleanup retains trust and respects the pass cap ---\n");
{
  const files: Record<string, string> = {
    "codeterm-foreign.config.toml": "# codeterm-instance: foreign\n[projects.'C:\\Foreign']\ntrust_level = 'trusted'\n",
    "codeterm-deadbeef.config.toml": "[projects.'C:\\Legacy']\ntrust_level = 'trusted'\n",
    "codeterm-user.config.toml": "developer_instructions = 'user'",
  };
  for (let index = 0; index < 201; index++) {
    files[`codeterm-${index.toString(16).padStart(8, "0")}.config.toml`] = "developer_instructions = 'old'";
  }
  const { host } = makeHost(files);
  const first = manageCodexProfiles(host, [], "owner", true, true, ["foreign"]);
  assert.equal(first.removed.length, 200);
  assert.equal(files["codeterm-foreign.config.toml"], undefined);
  assert.equal(files["codeterm-deadbeef.config.toml"], undefined);
  assert.ok(files["config.toml"].includes("C:\\Foreign"));
  assert.ok(files["config.toml"].includes("C:\\Legacy"));
  const second = manageCodexProfiles(host, [], "owner", true, true, ["foreign"]);
  assert.equal(second.removed.length, 3);
  assert.ok(files["codeterm-user.config.toml"]);
}

console.log("\n--- refused removals do not starve later reclaimable files ---\n");
{
  const files: Record<string, string> = {};
  for (let index = 0; index < 201; index++) {
    files[`codeterm-${index.toString(16).padStart(8, "0")}.config.toml`] = "# codeterm-instance: owner";
  }
  const { host } = makeHost(files);
  const remove = host.fs.removeFile;
  host.fs.removeFile = (path) => path.endsWith("codeterm-000000c8.config.toml") && remove(path);
  eq(manageCodexProfiles(host, [], "owner", true, false).removed,
    ["codeterm-000000c8.config.toml"], "the cap counts removals rather than refused candidates");
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

console.log("\n--- changed ownership between eligibility and trust carry fails closed ---\n");
for (const release of [false, true]) {
  const name = "codeterm-deadbeef.config.toml";
  const { host, files, removed } = makeHost({ [name]: "# codeterm-instance: owner" });
  const read = host.fs.readFile;
  let reads = 0;
  host.fs.readFile = (path) => {
    if (path.endsWith(name) && ++reads === 3) files[name] = "# codeterm-instance: foreign";
    return read(path);
  };
  if (release) assert.equal(releaseCodexProfile(host, "deadbeef", "owner"), false);
  else eq(manageCodexProfiles(host, [], "owner", true, false).removed, [], "changed owner is retained");
  eq(removed, [], "ownership change prevents deletion");
  assert.equal(files[name], "# codeterm-instance: foreign");
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

console.log("\n--- answered trust carries only this tab and instance before release ---\n");
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
  assert.equal(carryCodexProjectTrustForTab(host, "live-tab", "instance-dev"), true,
    "the answered tab can persist trust while its profile remains live");
  assert.ok(files["config.toml"].includes("[projects.'C:\\Users\\test\\TeamProject']"),
    "the live tab trust is immediately available to sibling and later Codex launches");
  assert.ok(!files["config.toml"].includes("C:\\Foreign"), "does not collect another instance's profile trust");
  assert.ok(!files["config.toml"].includes("C:\\Legacy"), "does not collect unmarked profile trust");
  assert.ok(files["config.toml"].startsWith(config), "hooks and trusted_hash bytes remain unchanged");
  assert.equal(carryCodexProjectTrustForTab(host, "foreign-tab", "instance-dev"), false,
    "does not carry a sibling instance's profile even when given its tab id");
}

console.log("\n--- live owner answer makes an already-running sibling eligible ---\n");
{
  const liveProfile = [
    "# codeterm-instance: instance-dev",
    "",
    "[projects.'C:\\Users\\test\\SharedFolder']",
    'trust_level = "trusted"',
    "",
  ].join("\r\n");
  const { host, files } = makeHost({
    "codeterm-owner.config.toml": liveProfile,
    "config.toml": "[hooks.state]\r\ntrusted_hash = \"sha256:unchanged\"\r\n",
  });
  assert.equal(isCodexDirectoryTrusted(host, "C:\\Users\\test\\SharedFolder"), false,
    "the sibling is not authorized before the owner's answer is merged");
  assert.equal(carryCodexProjectTrustForTab(host, "owner", "instance-dev"), true,
    "the answered live profile is merged without waiting for release");
  assert.equal(isCodexDirectoryTrusted(host, "c:/users/test/sharedfolder"), true,
    "the sibling's exact folder is recognized as owner-trusted immediately after merge");
  assert.equal(isCodexDirectoryTrusted(host, "C:\\Users\\test\\Neighbor"), false,
    "a sibling in a different folder remains ineligible");
  assert.ok(files["config.toml"].includes('trusted_hash = "sha256:unchanged"'),
    "the hook trust hash stays unchanged during promotion");
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
