import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
const quoted: string[] = [];
let instructionFile = "";
let instructionPath = "";
const context = {
  host: {
    manifestJson: () => "null",
    platform: () => "windows",
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
const command = context.module.exports.default.buildResumeCommandWithContext({
  sessionId: uuid,
  skipPermissions: true,
  agentName: "restored-agent-name",
  instanceId: "instance-restored-test",
  systemPrompt: instructions,
  args,
});

assert.equal(instructionFile, instructions);
assert(command.includes("codex resume"));
assert(command.includes("--profile $ctProfileName"));
assert(command.includes("launch-artifacts/release"));
assert(command.includes("instance-restored-test"));
assert(command.includes("--dangerously-bypass-approvals-and-sandbox"));
assert(command.includes("--dangerously-bypass-hook-trust"));
assert(!command.includes(instructions));
assert(quoted.includes(uuid));
assert(quoted.includes(instructionPath));
for (const argument of args) {
  assert(quoted.includes(argument));
  assert(command.includes(`ARG(${argument})`));
}
