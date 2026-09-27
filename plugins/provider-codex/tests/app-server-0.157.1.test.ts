import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const home = "C:\\codex-fixture";
const codexHome = `${home}\\.codex`;
const hooksPath = `${codexHome}\\hooks.json`;
const configPath = `${codexHome}\\config.toml`;
const sessionKey = `${hooksPath}:session_start:0:0`;
const stopKey = `${hooksPath}:stop:0:0`;
const fixturePath = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "app-server-0.157.1.json");
const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8")) as Record<string, any>;
const fixtureText = JSON.stringify(fixture)
  .replaceAll("<CODEX_HOME>", JSON.stringify(codexHome).slice(1, -1))
  .replaceAll("<WORKING_DIR>", JSON.stringify(`${home}\\work`).slice(1, -1));
const wire = JSON.parse(fixtureText) as Record<string, any>;

const config = [
  `[hooks.state.'${sessionKey}']`,
  'trusted_hash = "sha256:prior-session"',
  `[hooks.state.'${stopKey}']`,
  'trusted_hash = "sha256:prior-stop"',
  "[hooks.state.'C:\\other\\hooks.json:stop:0:0']",
  'trusted_hash = "sha256:foreign"',
  "",
].join("\n");

const files = new Map<string, string>([[configPath, config]]);
const calls: any[] = [];
const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");

const host = {
  manifestJson: () => "null",
  homeDir: () => home,
  platform: () => "windows",
  unixNowMs: () => 1_000,
  envGet: () => null,
  log: () => {},
  path: { equal: (left: string, right: string) => left.toLowerCase() === right.toLowerCase() },
  shell: { quoteFor: (value: unknown) => String(value), quote: (value: unknown) => String(value) },
  exec: (optsJson: string) => {
    const options = JSON.parse(optsJson);
    calls.push(options);
    const conversation = options.conversation;
    assert.equal(options.bin, "codex");
    assert.deepEqual(options.args, ["app-server"]);
    assert.deepEqual(conversation[0].awaitResponseIds, [1]);
    assert.equal(JSON.parse(conversation[0].stdin).method, "initialize");
    assert.deepEqual(JSON.parse(conversation[0].stdin).params.capabilities, { experimentalApi: true });
    const output: unknown[] = [{ ...wire.initialize }];
    for (const line of String(conversation[1].stdin).split("\n").filter(Boolean)) {
      const request = JSON.parse(line);
      if (request.method === "initialized") continue;
      if (request.method === "hooks/list") {
        assert.deepEqual(request.params, { cwds: [home] });
        const listing = calls.some((call) => String(call.conversation[1].stdin).includes("config/batchWrite"))
          ? wire.hooksListTrusted
          : wire.hooksListUntrusted;
        output.push({ ...listing, id: request.id });
      } else if (request.method === "config/batchWrite") {
        assert.equal(request.params.edits[0].keyPath, "hooks.state");
        assert.equal(request.params.edits[0].mergeStrategy, "upsert");
        assert.equal(request.params.reloadUserConfig, true);
        const values = request.params.edits[0].value;
        let current = files.get(configPath) || "";
        current = current.replace("sha256:prior-session", values[sessionKey]?.trusted_hash || "sha256:prior-session");
        current = current.replace("sha256:prior-stop", values[stopKey]?.trusted_hash || "sha256:prior-stop");
        files.set(configPath, current);
        output.push({ ...wire.configBatchWrite, id: request.id });
      }
    }
    return JSON.stringify({ code: 0, stdout: output.map((row) => JSON.stringify(row)).join("\n"), stderr: "" });
  },
  fs: {
    fileExists: (file: string) => files.has(file),
    readFile: (file: string) => files.get(file) ?? null,
    readJson: (file: string) => {
      try { return JSON.parse(files.get(file) || "{}"); } catch { return null; }
    },
    writeFile: (file: string, contents: string) => { files.set(file, contents); return true; },
    writeJson: (file: string, value: unknown) => { files.set(file, JSON.stringify(value)); return true; },
    removeFile: (file: string) => files.delete(file),
  },
};

const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
new vm.Script(fs.readFileSync(pluginPath, "utf8"), { filename: pluginPath }).runInNewContext(context);
const plugin = context.module.exports.default;

const install = plugin.installIntegration(7686, { consent: true });
assert.equal(install.ok, true);
const before = plugin.checkIntegration();
assert.equal(before.hook_trust_state, "unknown");
assert.equal(before.integration_state, "unknown");
assert.equal(calls.length, 0, "status reads the cached verdict and never starts app-server");

const startup = plugin.ensureIntegrationOnStartup(7686);
assert.equal(startup.actionable, true);
assert.equal(plugin.checkIntegration().hook_trust_state, "stale", JSON.stringify(startup));
assert.equal(plugin.checkIntegration().integration_state, "untrusted_hash");
assert.deepEqual(calls[0].conversation[1].awaitResponseIds, [2]);
assert.deepEqual(
  String(calls[0].conversation[1].stdin).split("\n").filter(Boolean).map((line: string) => JSON.parse(line).method),
  ["initialized", "hooks/list"],
);

const grant = plugin.grantIntegrationTrust(["session_start", "stop"]);
assert.equal(grant.granted, true, JSON.stringify(grant));
assert.equal(grant.wrote, true);
assert.deepEqual(calls[1].conversation[1].awaitResponseIds, [2]);
assert.deepEqual(
  String(calls[1].conversation[1].stdin).split("\n").filter(Boolean).map((line: string) => JSON.parse(line).method),
  ["initialized", "hooks/list"],
);
assert.deepEqual(calls[2].conversation[1].awaitResponseIds, [2, 3]);
assert.deepEqual(
  String(calls[2].conversation[1].stdin).split("\n").filter(Boolean).map((line: string) => JSON.parse(line).method),
  ["initialized", "config/batchWrite", "hooks/list"],
);
assert.equal(files.get(configPath)?.includes('trusted_hash = "sha256:foreign"'), true);
assert.equal(calls.length, 3, "0.157.1 completes status listing, grant, and verification in three sessions");

console.log("PASS Codex 0.157.1 app-server fixture binds hook listing and trust grant");
