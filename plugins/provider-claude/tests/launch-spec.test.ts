import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import type { LaunchParams, PluginModule } from "@codeterm/plugin-sdk";

const source = fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8");
let cases = 0;
for (const platform of ["windows", "linux", "macos"]) {
  for (const skipPermissions of [false, true]) {
    const files = new Map<string, string>();
    let enabled = true;
    const context = {
      module: { exports: {} as { default: PluginModule } },
      host: {
        platform: () => platform,
        manifestJson: () => JSON.stringify({ commands: enabled ? { launchSpec: { program: "claude" } } : {} }),
        log: () => {},
        fs: {
          writeTempFile(body: string, suffix: string) {
            const path = `/fixture/${files.size}${suffix}`;
            files.set(path, body);
            return path;
          },
        },
      },
    };
    vm.createContext(context);
    vm.runInContext(source, context);
    const plugin = context.module.exports.default;
    assert.equal(plugin.starterPromptText!("no_starter"), undefined);
    assert.equal(typeof plugin.starterPromptText!("assigned_task"), "string");
    assert.equal(plugin.buildLaunchSpec!({ resume: null }), null);
    const args = ["--resume", "fixture-session", "--model", "two words; $value 😀"];
    const params: LaunchParams = {
      args, skipPermissions, agentName: "name 'quoted' 😀", sessionId: "fixture-id",
      task: "opaque task $x; `command` 😀", starterPrompt: "no_starter",
    };
    const spec = JSON.parse(JSON.stringify(plugin.buildLaunchSpec!(params)!));
    assert.equal(spec.program, "claude");
    assert.deepEqual(spec.envSet, {});
    assert.deepEqual(spec.envUnset, ["CLAUDECODE"]);
    assert.deepEqual(spec.args.slice(0, skipPermissions ? 9 : 8), [
      ...(skipPermissions ? ["--dangerously-skip-permissions"] : []),
      "--agent", params.agentName, "--session-id", params.sessionId, ...args,
    ]);
    assert.deepEqual(spec.args.slice(-4).filter((_: string, index: number) => index % 2 === 0),
      ["--settings", "--append-system-prompt-file"]);
    assert.deepEqual(JSON.parse(files.get(spec.args[spec.args.length - 3])!), { showThinkingSummaries: true });
    assert.ok(files.get(spec.args.at(-1))!.includes(params.task!));
    assert.equal(files.get(spec.args.at(-1))!.startsWith("\uFEFF"), platform === "windows");
    assert.deepEqual(args, params.args);
    const resumed = JSON.parse(JSON.stringify(plugin.buildLaunchSpec!({ resume: {
      sessionId: "own-session", args: ["--model", "fixture-model"], skipPermissions,
      systemPrompt: "opaque resume context 😀",
    } })!));
    assert.deepEqual(resumed.args.slice(0, skipPermissions ? 5 : 4), [
      ...(skipPermissions ? ["--dangerously-skip-permissions"] : []),
      "--resume", "own-session", "--model", "fixture-model",
    ]);
    assert.ok(!resumed.args.includes("--session-id"));
    assert.ok(!resumed.args.includes("--"));
    enabled = false;
    assert.equal(plugin.buildLaunchSpec!(params), null);
    cases++;
  }
}
console.log(`${cases} direct launch argv/environment fixtures passed`);
