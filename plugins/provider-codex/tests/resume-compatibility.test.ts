import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

interface Commands {
  buildLaunchCommand(params: Record<string, unknown>): string;
  buildResumeCommand(sessionId: string, skipPermissions?: boolean): string;
  buildResumeCommandWithContext(params: Record<string, unknown>): string;
}

const root = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-codex-contract-"));
const bin = path.join(root, "bin");
const logPath = path.join(root, "calls.jsonl");
const instructionsPath = path.join(root, "instructions.md");
const platform = process.platform === "win32" ? "windows" : "linux";
const uuid = "11111111-1111-1111-1111-111111111111";
const modelArgs = ["-m", "gpt-6-sol", "-c", 'model_reasoning_effort="high"'];
const pluginPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../plugin.js");
const source = fs.readFileSync(pluginPath, "utf8");

function loadPlugin(target = platform): Commands {
  const context = {
    host: {
      manifestJson: () => "null",
      platform: () => target,
      shell: {
        quoteFor: (value: unknown) => target === "windows"
          ? `'${String(value).replace(/'/g, "''")}'`
          : `'${String(value).replace(/'/g, "'\\''")}'`,
      },
      fs: {
        writeTempFile: (contents: string) => {
          fs.writeFileSync(instructionsPath, contents);
          return instructionsPath;
        },
      },
    },
    module: { exports: {} as { default: Commands } },
    exports: {},
  };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

function execute(command: string, supported: boolean, actualBinary?: string) {
  fs.writeFileSync(logPath, "");
  const env = { ...process.env };
  for (const name of Object.keys(env)) {
    if (name.startsWith("CODETERM_")) delete env[name];
  }
  Object.assign(env, {
    PATH: `${bin}${path.delimiter}${process.env.PATH || ""}`,
    USERPROFILE: root,
    CODEX_HOME: path.join(root, ".codex"),
    CODETERM_TAB_ID: "isolated-contract-fixture",
    CODETERM_TEST_CODEX_LOG: logPath,
    CODETERM_TEST_CODEX_SUPPORT: String(supported),
    CODETERM_TEST_CODEX_ACTUAL: actualBinary || "",
  });
  const result = platform === "windows"
    ? spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], { env, cwd: root, encoding: "utf8" })
    : spawnSync("/bin/sh", ["-c", command], { env, cwd: root, encoding: "utf8" });
  assert.ifError(result.error);
  const calls = fs.readFileSync(logPath, "utf8").trim().split("\n").filter(Boolean)
    .map((line): string[] => JSON.parse(line));
  return { ...result, calls };
}

try {
  fs.mkdirSync(bin);
  fs.mkdirSync(path.join(root, ".codex"));
  const shimPath = path.join(bin, "codex-shim.cjs");
  fs.writeFileSync(shimPath, `
const fs = require('node:fs');
const { spawnSync } = require('node:child_process');
const args = process.argv.slice(2);
fs.appendFileSync(process.env.CODETERM_TEST_CODEX_LOG, JSON.stringify(args) + '\\n');
if (process.env.CODETERM_TEST_CODEX_ACTUAL) {
  if (!args.includes('--help')) args.push('--help');
  const result = spawnSync(process.env.CODETERM_TEST_CODEX_ACTUAL, args, { env: process.env, encoding: 'utf8' });
  if (result.error) throw result.error;
  process.stdout.write(result.stdout);
  process.stderr.write(result.stderr);
  process.exit(result.status ?? 1);
}
if (args.includes('--no-daemon') && process.env.CODETERM_TEST_CODEX_SUPPORT !== 'true') {
  process.stderr.write("error: unexpected argument '--no-daemon' found\\n");
  process.exit(2);
}
`);
  if (platform === "windows") {
    fs.writeFileSync(path.join(bin, "codex.cmd"), `@echo off\r\n"${process.execPath}" "${shimPath}" %*\r\n`);
  } else {
    fs.writeFileSync(path.join(bin, "codex"), `#!/bin/sh\nexec '${process.execPath.replace(/'/g, "'\\''")}' '${shimPath.replace(/'/g, "'\\''")}' "$@"\n`, { mode: 0o755 });
  }

  const plugin = loadPlugin();
  const resumes = [
    plugin.buildResumeCommand(uuid, false),
    plugin.buildResumeCommand(uuid, true),
    plugin.buildResumeCommandWithContext({ sessionId: uuid, instanceId: "fixture", args: modelArgs, systemPrompt: "opaque-context-token" }),
    plugin.buildResumeCommandWithContext({ sessionId: uuid, instanceId: "fixture", skipPermissions: true, args: modelArgs }),
  ];
  for (const [index, command] of resumes.entries()) {
    const rejected = execute(command, false);
    assert.notEqual(rejected.status, 0);
    assert.deepEqual(rejected.calls, [["resume", "--no-daemon", "--help"]]);
    assert(rejected.stderr.includes("0.156.0+"));
    assert(rejected.stderr.includes("codex update"));
    assert.equal(fs.existsSync(path.join(root, ".codex/codeterm-isolated-contract-fixture.config.toml")), false);

    const accepted = execute(command, true);
    assert.equal(accepted.status, 0, accepted.stderr);
    assert.equal(accepted.calls.length, 2);
    assert.deepEqual(accepted.calls[0], ["resume", "--no-daemon", "--help"]);
    const args = accepted.calls[1];
    assert(args.includes(uuid));
    assert.equal(args.filter((arg) => arg === "--no-daemon").length, 1);
    assert(!args.some((arg) => arg.includes("opaque-context-token")));
    for (const flag of ["--dangerously-bypass-approvals-and-sandbox", "--dangerously-bypass-hook-trust"]) {
      assert.equal(args.includes(flag), index === 1 || index === 3);
    }
    if (index >= 2) assert.deepEqual(args.slice(args.indexOf(uuid) + 2, args.indexOf(uuid) + 2 + modelArgs.length), modelArgs);
    fs.rmSync(path.join(root, ".codex/codeterm-isolated-contract-fixture.config.toml"), { force: true });
  }

  for (const skipPermissions of [false, true]) {
    const fresh = plugin.buildLaunchCommand({ skipPermissions, args: modelArgs, task: "opaque-task", starterPrompt: "no_starter", instanceId: "fixture" });
    const result = execute(fresh, false);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.calls.length, 1);
    assert(!result.calls[0].includes("--no-daemon"));
    assert.equal(result.calls[0].includes("--dangerously-bypass-approvals-and-sandbox"), skipPermissions);
    fs.rmSync(path.join(root, ".codex/codeterm-isolated-contract-fixture.config.toml"), { force: true });
  }

  const windows = loadPlugin("windows").buildResumeCommandWithContext({ sessionId: uuid, instanceId: "fixture" });
  assert(windows.indexOf("codex resume --no-daemon --help") < windows.indexOf("$ctTabId ="));
  assert(windows.includes("catch {}"));

  const actualBinary = process.env.CODETERM_TEST_CODEX_BINARY;
  if (actualBinary) {
    assert(path.isAbsolute(actualBinary));
    const expectedSupport = process.env.CODETERM_TEST_CODEX_EXPECT_SUPPORT !== "false";
    for (const command of [plugin.buildLaunchCommand({ args: modelArgs }), ...resumes]) {
      const result = execute(command, expectedSupport, actualBinary);
      const isResume = resumes.includes(command);
      assert.equal(result.status === 0, !isResume || expectedSupport, result.stderr);
      assert.equal(result.calls.length, !isResume || !expectedSupport ? 1 : 2);
    }
    const unsupported = spawnSync(actualBinary, ["resume", "--codeterm-unsupported-fixture", "--help"], { encoding: "utf8" });
    assert.ifError(unsupported.error);
    assert.notEqual(unsupported.status, 0);
    console.log(`Real CLI parser contract verified: ${actualBinary} (direct resume support=${expectedSupport})`);
  }
  console.log("Codex resume compatibility: rejected before side effects; supported argv and fresh launches preserved");
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
