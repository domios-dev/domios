import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import { spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

interface Spec { program: string; args: string[]; envSet: Record<string, string>; envUnset: string[] }
interface Provider {
  buildLaunchSpec(params: Record<string, unknown>): Spec | null;
  buildLaunchCommand(params: Record<string, unknown>): string;
  buildResumeCommandWithContext(params: Record<string, unknown>): string;
}
const directory = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(directory, "../plugin.js"), "utf8");
const manifest = fs.readFileSync(path.join(directory, "../plugin.json"), "utf8");
const exclusions = JSON.parse(manifest).capabilities.agentRuntime.inheritedEnvironmentExclusions;
const bash = process.platform === "win32" ? path.join(process.env.ProgramFiles || "C:/Program Files", "Git/bin/bash.exe") : "bash";
function load(platform = "linux", version: string | null = "0.159.0"): Provider {
  const context = {
    host: {
      platform: () => platform,
      manifestJson: () => manifest,
      providerVersionJson: () => JSON.stringify({ status: version ? "detected" : "failed", version }),
      shell: { quoteFor: (value: string) => "'" + value.replace(/'/g, "'\\''") + "'" },
    },
    module: { exports: {} as { default: Provider } }, exports: {},
  };
  new vm.Script(source).runInNewContext(context);
  return context.module.exports.default;
}
function plain<T>(value: T): T { return JSON.parse(JSON.stringify(value)); }
const provider = load();
const task = "opaque 'quoted' $value `command` 😀\nsecond line\n\n";
for (const skipPermissions of [false, true]) {
  for (const assigned of [false, true]) {
    for (const questions of [false, true]) {
      const hostOwned = { questions, todos: false };
      const params = { skipPermissions, hostOwned, args: ["-m", "fixture model"], ...(assigned ? { task, launchMarker: "fixture-launch-nonce", starterPromptText: "opaque starter" } : {}) };
      const spec = provider.buildLaunchSpec(params)!;
      assert(spec);
      assert.deepEqual(plain(spec.envUnset), exclusions);
      assert.deepEqual(plain(spec.envSet), {});
      assert.equal(spec.program, "codex");
      if (assigned) assert(spec.args.includes("developer_instructions=" + task + "\n\n<!-- codeterm-launch-marker: fixture-launch-nonce -->"));
      assert.equal(spec.args.includes("--dangerously-bypass-hook-trust"), skipPermissions);
      const questionFeatureIndex = spec.args.indexOf("--disable");
      if (questions) assert.deepEqual(spec.args.slice(questionFeatureIndex, questionFeatureIndex + 2), ["--disable", "default_mode_request_user_input"]);
      else assert.equal(questionFeatureIndex, -1);
      const resume = { sessionId: "fixture-session", skipPermissions, hostOwned, args: ["-m", "fixture model"] };
      const resumed = provider.buildLaunchSpec({ resume })!;
      assert(resumed.args.includes("--no-daemon"));
      const resumeQuestionFeatureIndex = resumed.args.indexOf("--disable");
      if (questions) assert.deepEqual(resumed.args.slice(resumeQuestionFeatureIndex, resumeQuestionFeatureIndex + 2), ["--disable", "default_mode_request_user_input"]);
      else assert.equal(resumeQuestionFeatureIndex, -1);
      for (const [launch, command] of [[spec, provider.buildLaunchCommand(params)], [resumed, provider.buildResumeCommandWithContext(resume)]] as const) {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), "codex-spec-"));
        try {
          const script = path.join(root, "parity.sh");
          fs.writeFileSync(script, 'codex() { printf "%s\\0" "$@"; printf "%s\\0" "${CODEX_THREAD_ID-unset}" "${CODEX_INTERNAL_ORIGINATOR_OVERRIDE-unset}"; }\n' + command);
          const result = spawnSync(bash, ["--noprofile", "--norc", script.replace(/\\/g, "/")], { encoding: "utf8", env: { ...process.env, CODEX_THREAD_ID: "foreign", CODEX_INTERNAL_ORIGINATOR_OVERRIDE: "foreign" } });
          assert.ifError(result.error);
          assert.equal(result.status, 0, result.stderr);
          assert.deepEqual(result.stdout.split("\0").slice(0, -1), [...launch.args, "unset", "unset"]);
        } finally { fs.rmSync(root, { recursive: true, force: true }); }
      }
    }
  }
}
for (const platform of ["windows"]) {
  assert.equal(load(platform).buildLaunchSpec({ task }), null);
  assert.equal(load(platform).buildLaunchSpec({}), null);
  assert.equal(load(platform).buildLaunchSpec({ resume: { sessionId: "fixture" } }), null);
}
for (const version of [null, "0.158.9", "0.159.0-pre", "invalid"]) {
  assert.equal(load("linux", version).buildLaunchSpec({ resume: { sessionId: "fixture" } }), null);
}
const mac = load("macos");
const overhead = ["codex", ...mac.buildLaunchSpec({})!.args].reduce((size, value) => size + Buffer.byteLength(value) + 1, 0);
const argument = "x".repeat(256 * 1024 - overhead - 1);
assert(mac.buildLaunchSpec({ args: [argument] }));
assert.equal(mac.buildLaunchSpec({ args: [argument + "x"] }), null);
assert.equal(mac.buildLaunchSpec({ args: [argument.slice(0, -1) + "é"] }), null);
assert(provider.buildLaunchSpec({ args: ["x".repeat(128 * 1024 - 1)] }));
assert.equal(provider.buildLaunchSpec({ args: ["x".repeat(128 * 1024)] }), null);
assert.equal(provider.buildLaunchSpec({ task: "x".repeat(128 * 1024) }), null);
assert.equal(provider.buildLaunchSpec({ args: ["embedded\0nul"] }), null);
console.log("launch-spec: 16 shell argv/env round trips, platform/version refusals, UTF-8 budget boundaries passed");
