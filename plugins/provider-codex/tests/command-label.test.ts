import assert from "node:assert/strict";
import { activityCommand, activityLabel, type ActivityTool } from "@codeterm/plugin-sdk";
import { runningCallStatus } from "../src/runtimeAbandonedTurns";

const pwsh = "C:\\Users\\User\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe";
const commands: [unknown, string][] = [
  [[pwsh, "-NoLogo", "-NoProfile", "-Command", "git status --short"], "git status"],
  [`"${pwsh}" -Command "Set-Location 'C:\\repo'; npx tsc --noEmit"`, "Checking types"],
  [["powershell.exe", "-c", "Get-Content C:\\repo\\README.md"], "Reading README.md"],
  [["/bin/bash", "-lc", "cd /repo && pnpm test"], "Running tests"],
  ["'/bin/sh' -c 'cat /repo/src/model.ts'", "Reading model.ts"],
  [["C:\\Program Files\\Git\\bin\\bash.exe", "-lc", "./build.sh check"], "Build: check"],
  [["cmd.exe", "/c", "git diff --stat"], "git diff"],
  ['cmd /d /s /c "pnpm test"', "Running tests"],
  ["tsc --noEmit", "Checking types"],
  ["pnpm exec tsc --noEmit", "Checking types"],
  [[pwsh, "-Command", 'Get-Content "C:\\repo\\my file.ts"'], "Reading my file.ts"],
  ["npx vitest run", "Running tests"],
  ["jest src/example.test.ts", "Running tests"],
  ["node --test tests/example.test.mjs", "Running tests"],
  ["./build.ps1 check", "Build: check"],
  ["bash ./build.sh test-one app example", "Build: test-one"],
  ['"C:\\Program Files\\Git\\cmd\\git.exe" status --short', "git status"],
  ["rg -n activity src", "Searching"],
  ["git -C /repo status --short", "git status"],
  ["grep needle src/file.ts", "Searching"],
  ["sed -n '1,20p' /repo/src/model.ts", "Reading model.ts"],
  ["cat '/repo/my file.ts'", "Reading my file.ts"],
  ["/usr/local/bin/custom inspect file.txt", "custom inspect file.txt"],
  ["echo first\necho second", "echo first"],
  ["echo alpha beta gamma delta epsilon zeta eta theta iota kappa lambda | sort | uniq", "echo alpha beta gamma delta epsilon zeta eta theta iota…"],
];

for (const [command, expected] of commands) {
  assert.equal(activityLabel({toolKind:"command",toolInput:{command}}), expected, JSON.stringify(command));
}

const tools: [ActivityTool, string][] = [
  [{toolName:"apply_patch",toolInput:"*** Begin Patch\n*** Update File: /repo/src/model.ts\n@@"}, "Editing model.ts"],
  [{toolName:"Edit",toolInput:{file_path:"C:\\repo\\model.ts"}}, "Editing model.ts"],
  [{toolKind:"edit",fileDiffs:[{path:"src/view.tsx"}]}, "Editing view.tsx"],
  [{toolName:"Read",toolInput:{file_path:"/repo/data.json"}}, "Reading data.json"],
  [{toolName:"server · tool",toolSummary:'{"key":"value"}'}, "server · tool"],
];
for (const [tool, expected] of tools) assert.equal(activityLabel(tool), expected);

const raw = `"${pwsh}" -Command "git status --short"`;
const row = {id:"call",seq:1,type:"tool_call" as const,content:"",toolKind:"command" as const,toolDetail:raw,toolFlowId:"turn",toolStatus:"running" as const};
assert.equal(runningCallStatus({activeTurnId:"turn",completed:[],items:{call:row},seq:1}), "git status");
assert.equal(activityCommand(row), raw);
assert.equal(runningCallStatus({activeTurnId:"other",completed:[],items:{call:row},seq:1}), undefined);
console.log(`PASS: ${commands.length + tools.length + 3} activity label fixtures and runtime checks`);
