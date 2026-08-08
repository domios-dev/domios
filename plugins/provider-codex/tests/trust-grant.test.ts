/**
 * Codex hook-trust grant mechanism tests (integration.trust_grant, DN-1).
 * Run: npx tsx plugins/provider-codex/tests/trust-grant.test.ts
 *
 * The app-server is STUBBED: every trusted_hash asserted below round-trips
 * from the stub's own hooks/list report — never a hash baked into a fixture
 * and never one computed by CodeTerm.
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

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

function loadCodexWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const HOME = "/tmp/home";
const HOOKS_PATH = `${HOME}/.codex/hooks.json`;
const CONFIG_PATH = `${HOME}/.codex/config.toml`;
const MANAGED_KEY = `${HOOKS_PATH}:session_start:0:0`;
const MANAGED_COMMAND = "curl ... /api/tabs/$CODETERM_TAB_ID/hook/session_start ...";

function fsHost(files: Map<string, string>) {
  return {
    manifestJson: () => "null",
    homeDir: () => HOME,
    platform: () => "linux",
    unixNowMs: () => 1_000_000,
    path: { equal: (a: string, b: string) => a === b },
    expandHome: (v: string) => v.replace(/^~/, HOME),
    envGet: () => null,
    log: () => {},
    shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
    fs: {
      fileExists: (p: string) => files.has(p),
      readFile: (p: string) => files.get(p) ?? null,
      readFileHead: () => null,
      readFileTail: () => null,
      readJson: (p: string) => {
        const value = files.get(p);
        if (value === undefined) return null;
        try { return JSON.parse(value); } catch { return null; }
      },
      writeFile: (p: string, value: string) => { files.set(p, value); return true; },
      writeJson: (p: string, value: unknown) => { files.set(p, JSON.stringify(value)); return true; },
      removeFile: (p: string) => files.delete(p),
      writeTempFile: () => "/tmp/task.md",
      walkDir: () => [],
    },
  };
}

interface StubOptions {
  files: Map<string, string>;
  /** Hash the stub reports on the first hooks/list. */
  reportedHash: string;
  /** Hash the stub reports on the post-write re-list (defaults to reportedHash). */
  reListHash?: string;
  /** Simulate an old CLI without the app-server subcommand. */
  rpcsAbsent?: boolean;
}

interface StubState {
  execCalls: number;
  batchWriteValues: Record<string, { trusted_hash: string }>[];
  trusted: Record<string, string>;
}

/** Models `codex app-server` over pre-composed stdin JSONL: answers each
 *  request in order and "exits". config/batchWrite mimics Codex's writer —
 *  appends exactly one [hooks.state."<key>"] table per upserted key. */
function makeAppServerStubExec(opts: StubOptions): { exec: (optsJson: string) => string; state: StubState } {
  const state: StubState = { execCalls: 0, batchWriteValues: [], trusted: {} };
  const exec = (optsJson: string): string => {
    state.execCalls++;
    const parsed = JSON.parse(optsJson);
    if (parsed.bin !== "codex" || !Array.isArray(parsed.args) || parsed.args[0] !== "app-server") {
      return JSON.stringify({ code: 1, stdout: "", stderr: `unexpected invocation ${parsed.bin} ${parsed.args}` });
    }
    if (typeof parsed.timeoutMs !== "number" || parsed.timeoutMs <= 0) {
      return JSON.stringify({ code: 1, stdout: "", stderr: "missing exit deadline" });
    }
    if (opts.rpcsAbsent) {
      return JSON.stringify({ code: 2, stdout: "", stderr: "error: unrecognized subcommand 'app-server'" });
    }
    let wroteThisSession = false;
    const out: string[] = [];
    const lines = String(parsed.stdin || "").split("\n");
    for (const line of lines) {
      if (!line.trim()) continue;
      const req = JSON.parse(line);
      if (typeof req.id !== "number") continue; // notifications
      if (req.method === "initialize") {
        out.push(JSON.stringify({ id: req.id, result: {} }));
      } else if (req.method === "hooks/list") {
        const hash = wroteThisSession ? (opts.reListHash ?? opts.reportedHash) : opts.reportedHash;
        const trustStatus = state.trusted[MANAGED_KEY] === hash ? "trusted" : "untrusted";
        out.push(JSON.stringify({
          id: req.id,
          result: {
            data: [{
              hooks: [
                { key: MANAGED_KEY, command: MANAGED_COMMAND, currentHash: hash, trustStatus },
                { key: `${HOOKS_PATH}:session_start:1:0`, command: "user-own-session-start.sh", currentHash: "sha256:user-hook-hash", trustStatus: "untrusted" },
              ],
            }],
          },
        }));
      } else if (req.method === "config/batchWrite") {
        const edit = req.params.edits[0];
        state.batchWriteValues.push(edit.value);
        let config = opts.files.get(CONFIG_PATH) ?? "";
        for (const key of Object.keys(edit.value)) {
          state.trusted[key] = edit.value[key].trusted_hash;
          config += `${config.length && !config.endsWith("\n") ? "\n" : ""}[hooks.state."${key}"]\ntrusted_hash = "${edit.value[key].trusted_hash}"\n`;
        }
        opts.files.set(CONFIG_PATH, config);
        wroteThisSession = true;
        out.push(JSON.stringify({ id: req.id, result: {} }));
      } else {
        out.push(JSON.stringify({ id: req.id, error: { code: -32601, message: "method not found" } }));
      }
    }
    return JSON.stringify({ code: 0, stdout: out.join("\n") + "\n", stderr: "" });
  };
  return { exec, state };
}

console.log("\n--- trust_grant: grant round-trip (Codex is the only hash authority) ---\n");

{
  const files = new Map<string, string>([[HOOKS_PATH, "{}"]]);
  // Runtime-generated hash: the assertion can only pass by round-tripping the
  // stub's reported value through the grant lane.
  const reportedHash = "sha256:codex-stub-" + Math.random().toString(36).slice(2);
  const stub = makeAppServerStubExec({ files, reportedHash });
  const host = { ...fsHost(files), exec: stub.exec };
  const p = loadCodexWithHost(host);

  const result = p.grantIntegrationTrust(["session_start"]);
  assert(result.granted === true, "trust_grant round-trip reports granted", JSON.stringify(result));
  assert(result.wrote === true, "trust_grant round-trip actually wrote trust");
  eq(stub.state.batchWriteValues.length, 1, "trust_grant issues exactly one config/batchWrite");
  eq(
    stub.state.batchWriteValues[0][MANAGED_KEY]?.trusted_hash,
    reportedHash,
    "trust_grant batchWrite value equals the hash the stub reported",
  );
  assert(
    (files.get(CONFIG_PATH) || "").includes(reportedHash),
    "trust_grant stored hash is the stub-reported hash",
  );
  eq(result.entries, [{ key: MANAGED_KEY, hash: reportedHash }], "trust_grant entries carry the reported hash");
}

console.log("\n--- trust_grant: scope negative control (foreign hooks.state untouched) ---\n");

{
  const foreignBlock = '[hooks.state."/home/user/.codex/hooks.json:stop:3:1"]\nenabled = false\ntrusted_hash = "sha256:user-owned-hash"\n';
  const files = new Map<string, string>([
    [HOOKS_PATH, "{}"],
    [CONFIG_PATH, foreignBlock],
  ]);
  const reportedHash = "sha256:codex-stub-" + Math.random().toString(36).slice(2);
  const stub = makeAppServerStubExec({ files, reportedHash });
  const p = loadCodexWithHost({ ...fsHost(files), exec: stub.exec });

  const result = p.grantIntegrationTrust(["session_start"]);
  assert(result.granted === true, "trust_grant scope control still grants", JSON.stringify(result));
  eq(
    Object.keys(stub.state.batchWriteValues[0] || {}),
    [MANAGED_KEY],
    "trust_grant batchWrite is scoped to exactly the managed key",
  );
  assert(
    (files.get(CONFIG_PATH) || "").startsWith(foreignBlock),
    "trust_grant leaves the foreign hooks.state block byte-identical",
    files.get(CONFIG_PATH),
  );
}

console.log("\n--- trust_grant: degradation negative control (RPCs absent => enable-only) ---\n");

{
  const files = new Map<string, string>([[HOOKS_PATH, "{}"]]);
  const stub = makeAppServerStubExec({ files, reportedHash: "sha256:never-served", rpcsAbsent: true });
  const p = loadCodexWithHost({ ...fsHost(files), exec: stub.exec });

  const install = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(install.ok === true, "trust_grant degradation: enable-only install succeeds", JSON.stringify(install));
  const preGrantConfig = files.get(CONFIG_PATH);

  const result = p.grantIntegrationTrust(["session_start"]);
  assert(result.granted === false && result.degraded === true, "trust_grant degrades when app-server RPCs are absent", JSON.stringify(result));
  eq(stub.state.batchWriteValues.length, 0, "trust_grant degradation never writes trust");
  eq(files.get(CONFIG_PATH), preGrantConfig, "trust_grant degradation leaves config bytes unchanged");
  const check = p.checkIntegration();
  eq(check.hook_trust_state, "missing", "trust_grant degradation keeps hook_trust_state missing (honest)");
}

console.log("\n--- trust_grant: re-verify failure restores config from snapshot ---\n");

{
  const foreignBlock = '[hooks.state."/home/user/.codex/hooks.json:stop:3:1"]\nenabled = false\ntrusted_hash = "sha256:user-owned-hash"\n';
  const files = new Map<string, string>([
    [HOOKS_PATH, "{}"],
    [CONFIG_PATH, foreignBlock],
  ]);
  const reportedHash = "sha256:codex-stub-" + Math.random().toString(36).slice(2);
  const stub = makeAppServerStubExec({
    files,
    reportedHash,
    reListHash: reportedHash + "-drifted",
  });
  const p = loadCodexWithHost({ ...fsHost(files), exec: stub.exec });
  const snapshotBytes = files.get(CONFIG_PATH);

  const result = p.grantIntegrationTrust(["session_start"]);
  assert(result.granted === false, "trust_grant re-verify drift reports the grant failed", JSON.stringify(result));
  assert(result.verify_failed === true, "trust_grant re-verify drift is classified verify_failed");
  assert(result.restored === true, "trust_grant re-verify drift restored the snapshot");
  eq(stub.state.batchWriteValues.length, 1, "trust_grant re-verify drift wrote before failing (mutating failure)");
  eq(files.get(CONFIG_PATH), snapshotBytes, "trust_grant re-verify failure restores exact pre-grant config bytes");
  const check = p.checkIntegration();
  assert(check.hook_trust_state !== "trusted", "trust_grant re-verify failure never fabricates trusted state");
}

console.log("\n--- trust_grant: trusted state reads back after a successful grant ---\n");

{
  const files = new Map<string, string>([[HOOKS_PATH, "{}"]]);
  const reportedHash = "sha256:codex-stub-" + Math.random().toString(36).slice(2);
  const stub = makeAppServerStubExec({ files, reportedHash });
  const p = loadCodexWithHost({ ...fsHost(files), exec: stub.exec });

  const install = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(install.ok === true, "trust_grant install before grant succeeds", JSON.stringify(install));
  const result = p.grantIntegrationTrust(["session_start"]);
  assert(result.granted === true, "trust_grant grant after install succeeds", JSON.stringify(result));
  const check = p.checkIntegration();
  eq(check.hook_trust_state, "trusted", "trust_grant checkIntegration reports trusted after the grant");
  assert(check.hooks_installed === true, "trust_grant hooks stay installed with granted trust");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
