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
  'trust_level = "trusted"',
  "",
].join("\r\n");

const hooks = [
  "[hooks.state]",
  "",
  "[hooks.state.'C:\\Users\\dev\\.codex\\hooks.json:session_start:0:0']",
  'trusted_hash = "sha256:keep-me"',
  "enabled = true",
  "",
].join("\r\n");

assert.equal(
  mergeTrustedProjectEntries(profile, hooks),
  hooks + "\r\n[projects.'d:\\developer\\trust-probe']\r\ntrust_level = \"trusted\"\r\n",
  "copies the exact Codex project key and leaves hook trust bytes untouched",
);
assert.equal(
  mergeTrustedProjectEntries(profile, "[projects.'d:\\developer\\trust-probe']\ntrust_level = 'read'\n"),
  "[projects.'d:\\developer\\trust-probe']\ntrust_level = \"trusted\"\n",
  "updates only the existing matching project trust setting",
);
assert.equal(
  mergeTrustedProjectEntries('developer_instructions = "brief"\n', hooks),
  hooks,
  "does not copy unrelated profile settings",
);

const configPath = "C:\\Users\\dev\\.codex\\config.toml";
const files = new Map<string, string>([["profile", profile], [configPath, hooks]]);
let lockHeld = false;
let atomicWrites = 0;
let lockId = "";
let concurrentWrite: (() => void) | null = () => {
  files.set(configPath, hooks + "[projects.'C:\\Concurrent']\r\ntrust_level = \"trusted\"\r\n");
};
const host: ProjectTrustCarryHost = {
  fs: {
    fileExists: (path) => files.has(path),
    readFile: (path) => files.get(path) ?? null,
    writeFileAtomic: (path, contents) => {
      assert.equal(lockHeld, true, "atomic config replacement stays inside the shared lock");
      atomicWrites++;
      files.set(path, contents);
      return true;
    },
    lockFile: () => {
      lockHeld = true;
      concurrentWrite?.();
      concurrentWrite = null;
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

assert.equal(carryCodexProjectTrust(host, "profile", configPath), true);
const carried = files.get(configPath)!;
assert.ok(carried.includes("[projects.'d:\\developer\\trust-probe']"), "keeps Codex's lowercased path casing");
assert.ok(carried.includes("[projects.'C:\\Concurrent']"), "re-reads and preserves a concurrent writer's entry");
assert.ok(!carried.includes("model_availability_nux"), "does not carry the TUI counter noise");
assert.ok(carried.includes('trusted_hash = "sha256:keep-me"'), "leaves recorded hook hashes byte-for-byte present");
assert.equal(carryCodexProjectTrust(host, "profile", configPath), true, "repeat carry is successful");
assert.equal(atomicWrites, 1, "repeat carry is idempotent and does not rewrite config");
assert.equal(lockHeld, false, "releases the shared file lock after each merge");

console.log("PASS Codex project trust carry-over and concurrent merge");
