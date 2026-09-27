import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
const quoted: string[] = [];
let instructionFile = "";
let instructionPath = "";
let platform = "windows";
const context = {
  host: {
    manifestJson: () => "null",
    platform: () => platform,
    shell: {
      quoteFor: (value: unknown) => {
        const argument = String(value);
        quoted.push(argument);
        return `ARG(${argument})`;
      },
    },
    fs: {
      writeTempFile: (contents: string) => {
        instructionFile = contents;
        instructionPath = "C:\\Temp\\restored instructions.md";
        return instructionPath;
      },
    },
  },
  console,
  module: { exports: {} as { default?: any } },
  exports: {},
};
new vm.Script(fs.readFileSync(pluginPath, "utf8"), { filename: pluginPath }).runInNewContext(context);

const uuid = "11111111-1111-1111-1111-111111111111";
const instructions = "opaque-restored-instructions-d83a";
const args = ["-m", "gpt-6-luna", "-c", 'model_reasoning_effort="xhigh"'];
const resume = context.module.exports.default.buildResumeCommandWithContext;
const launch = context.module.exports.default.buildLaunchCommand;
const simpleResume = context.module.exports.default.buildResumeCommand;
for (const command of [
  launch({}),
  launch({ task: "task", instanceId: "instance-test" }),
  simpleResume("session-test"),
  resume({ sessionId: "session-test" }),
]) {
  assert.equal((command.match(/-c check_for_update_on_startup=false/g) || []).length, 1);
}
const parsedArguments = (command: string) =>
  Array.from(command.matchAll(/ARG\(([^()]*)\)/g), ([, argument]) => argument);
const windowsCommand = resume({
  sessionId: uuid,
  skipPermissions: true,
  agentName: "restored-agent-name",
  instanceId: "instance-restored-test",
  systemPrompt: instructions,
  args,
});

assert.equal(instructionFile, "");
assert(windowsCommand.includes(" resume"));
assert.equal((windowsCommand.match(/-c check_for_update_on_startup=false/g) || []).length, 1);
assert(windowsCommand.includes("--profile $ctProfileName"));
assert(windowsCommand.includes("launch-artifacts/release"));
assert(windowsCommand.includes("instance-restored-test"));
assert(windowsCommand.includes("ReadAllLines($ctProfilePath)"));
assert(windowsCommand.includes("$ctOwnsPriorProfile"));
assert(windowsCommand.includes("-preserved.config.toml"));
assert(windowsCommand.includes("WriteAllText($ctProfilePath"));
assert(windowsCommand.includes("UTF8Encoding]::new($false)"));
assert(windowsCommand.includes("--no-daemon"));
assert(windowsCommand.includes("--dangerously-bypass-approvals-and-sandbox"));
assert(windowsCommand.includes("--dangerously-bypass-hook-trust"));
assert(!windowsCommand.includes("Set-Content"));
assert(!windowsCommand.includes(instructions));
assert(quoted.includes(uuid));
assert(!quoted.includes(instructionPath));
assert.deepEqual(parsedArguments(windowsCommand).slice(-args.length - 1), [uuid, ...args]);
for (const argument of args) {
  assert(quoted.includes(argument));
  assert(windowsCommand.includes(`ARG(${argument})`));
}

platform = "linux";
quoted.length = 0;
const unixCommand = resume({ sessionId: uuid, args, systemPrompt: instructions });
assert(unixCommand.includes(" resume"));
assert.equal((unixCommand.match(/-c check_for_update_on_startup=false/g) || []).length, 1);
assert(unixCommand.includes("--no-daemon"));
assert(!unixCommand.includes("--profile"));
assert(!unixCommand.includes(instructions));
assert.deepEqual(parsedArguments(unixCommand), [uuid, ...args]);
for (const argument of args) {
  assert(unixCommand.includes(`ARG(${argument})`));
}
