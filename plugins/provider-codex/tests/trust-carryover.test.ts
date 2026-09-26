import assert from "node:assert/strict";
import {
  carryCodexProjectTrust,
  mergeTrustedProjectEntries,
  type ProjectTrustCarryHost,
} from "../src/trust-carryover";

const profile = [
  "# codeterm-instance: dev-tab",
  'developer_instructions = "opaque instructions"',
  "",
  "[tui.model_availability_nux]",
  "shown = 4",
  "",
  "[projects.'d:\\developer\\trust-probe']",
  'trust_level = "trusted" # accepted in Codex',
  "",
].join("\r\n");
const untrustedProfile = "[projects.'C:\\Untrusted']\r\ntrust_level = 'untrusted'\r\n";
const configPath = "C:\\Users\\dev\\.codex\\config.toml";
const config = [
  "# user config",
  "[hooks.state]",
  "",
  "[hooks.state.'C:\\Users\\dev\\.codex\\hooks.json:session_start:0:0']",
  'trusted_hash = "sha256:keep-me"',
  "enabled = true",
  "",
].join("\r\n");

assert.equal(
  mergeTrustedProjectEntries(profile, config),
  config + "\r\n[projects.'d:\\developer\\trust-probe']\r\ntrust_level = \"trusted\" # accepted in Codex\r\n",
  "copies Codex's exact project key and assignment while preserving all other config bytes",
);
assert.equal(
  mergeTrustedProjectEntries(profile, '[projects."D:\\\\DEVELOPER\\\\trust-probe"]\ntrust_level = "untrusted"\n'),
  '[projects."D:\\\\DEVELOPER\\\\trust-probe"]\ntrust_level = "untrusted"\n',
  "recognizes the same Windows path under a different quote style and case without adding a duplicate",
);
assert.equal(
  mergeTrustedProjectEntries("[projects.'/Workspace/Project']\ntrust_level = 'trusted'\n", "[projects.'/workspace/project']\ntrust_level = 'untrusted'\n"),
  "[projects.'/workspace/project']\ntrust_level = 'untrusted'\n\n[projects.'/Workspace/Project']\ntrust_level = 'trusted'\n",
  "keeps case-distinct project paths separate on case-sensitive filesystems",
);
assert.equal(
  mergeTrustedProjectEntries(profile, "[projects]\n'd:\\developer\\trust-probe' = { trust_level = 'untrusted' }\n"),
  "[projects]\n'd:\\developer\\trust-probe' = { trust_level = 'untrusted' }\n",
  "recognizes project entries nested under the projects parent table",
);
assert.equal(
  mergeTrustedProjectEntries(profile, 'projects = { "d:\\\\developer\\\\trust-probe" = { trust_level = "untrusted" } }\n'),
  'projects = { "d:\\\\developer\\\\trust-probe" = { trust_level = "untrusted" } }\n',
  "recognizes project entries in an inline projects table",
);
assert.equal(
  mergeTrustedProjectEntries(profile, "[projects.'d:\\developer\\trust-probe']\ntrust_level = 'untrusted'\n"),
  "[projects.'d:\\developer\\trust-probe']\ntrust_level = 'untrusted'\n",
  "never overwrites an explicit existing trust decision",
);
assert.equal(
  mergeTrustedProjectEntries(untrustedProfile, ""),
  "[projects.'C:\\Untrusted']\r\ntrust_level = 'untrusted'\r\n",
  "carries Codex's explicit untrusted answer as well as trusted answers",
);
assert.equal(
  mergeTrustedProjectEntries(profile, "projects = { 'C:\\Other' = { trust_level = 'trusted' } }\n"),
  null,
  "fails closed when adding a project table would conflict with an inline projects table",
);
assert.equal(
  mergeTrustedProjectEntries('developer_instructions = "brief"\n', config),
  config,
  "does not copy developer instructions or Codex's TUI counter noise",
);

function hostFor(files: Map<string, string>, configReads: { count: number } = { count: 0 }): ProjectTrustCarryHost {
  let lockHeld = false;
  let lockId = "";
  return {
    log: () => {},
    fs: {
      fileExists: (path) => files.has(path),
      readFile: (path) => {
        if (path === configPath) configReads.count++;
        return files.get(path) ?? null;
      },
      writeFileAtomic: (path, contents) => {
        assert.equal(lockHeld, true, "atomic config replacement stays inside the short shared merge lock");
        files.set(path, contents);
        return true;
      },
      lockFile: () => {
        lockHeld = true;
        lockId = `lock-${lockId.length + 1}`;
        return lockId;
      },
      unlockFile: (id) => {
        assert.equal(id, lockId, "releases the lock it acquired");
        lockHeld = false;
        return true;
      },
    },
  };
}

{
  const files = new Map<string, string>([["profile", profile], [configPath, config]]);
  const reads = { count: 0 };
  const host = hostFor(files, reads);
  assert.equal(carryCodexProjectTrust(host, "profile", configPath), true);
  const carried = files.get(configPath)!;
  assert.ok(carried.includes("[projects.'d:\\developer\\trust-probe']"), "preserves Codex's exact path casing");
  assert.ok(!carried.includes("model_availability_nux"), "does not carry TUI counter noise");
  assert.ok(carried.includes('trusted_hash = "sha256:keep-me"'), "leaves recorded hook hashes byte-identical");
  assert.equal(carryCodexProjectTrust(host, "profile", configPath), true, "repeat carry is successful");
  assert.equal(files.get(configPath), carried, "repeat carry is byte-idempotent");
}

{
  const concurrent = config + "[projects.'C:\\Concurrent']\r\ntrust_level = \"trusted\"\r\n";
  const files = new Map<string, string>([["profile", profile], [configPath, config]]);
  let atomicWrites = 0;
  const reads = { count: 0 };
  const host = hostFor(files, reads);
  const originalRead = host.fs.readFile;
  host.fs.readFile = (path) => {
    if (path === configPath && reads.count === 1) files.set(configPath, concurrent);
    return originalRead(path);
  };
  host.fs.writeFileAtomic = () => { atomicWrites++; return true; };
  assert.equal(carryCodexProjectTrust(host, "profile", configPath), false,
    "aborts rather than overwriting a writer that changed config after the initial read");
  assert.equal(files.get(configPath), concurrent, "retains the concurrent writer's project entry");
  assert.equal(atomicWrites, 0, "does not atomically write after detecting concurrent change");
}

{
  const files = new Map<string, string>([["profile", profile], [configPath, config]]);
  let warning = "";
  const host = hostFor(files);
  host.fs.lockFile = () => null;
  host.log = (_level, message) => { warning = message; };
  assert.equal(carryCodexProjectTrust(host, "profile", configPath), false,
    "lock contention leaves the profile in place for a later release attempt");
  assert.equal(files.get(configPath), config, "lock contention leaves shared config untouched");
  assert.ok(warning.includes("lock is busy"), "lock contention records a warning");
}

console.log("PASS Codex project trust carry-over, TOML structure checks, and concurrency guard");
