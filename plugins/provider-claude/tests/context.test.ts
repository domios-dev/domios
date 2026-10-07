import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { parseContextUsage, statuslineLaunchArgs, statuslineSettings, STATUSLINE_FLAG } from "../src/context";

let checks = 0;
function check(run: () => void) { run(); checks++; }
const record = { session_id: "secret", cwd: "private", cost: {total_cost_usd: 1.25},
  context_window: {context_window_size: 200000, current_usage: {
    input_tokens: 100, cache_creation_input_tokens: 20, cache_read_input_tokens: 30, output_tokens: 999,
  }} };
check(() => assert.deepEqual(parseContextUsage(record), {
  used_tokens:150, window_tokens:200000,cost_usd:1.25,source:"claude-statusline",
}));
for (const input of [-1, 0.5, 10_000_001, "1", null]) {
  check(() => assert.equal(parseContextUsage({type:"assistant",message:{usage:{input_tokens:input}}}), null));
}
check(() => assert.equal(parseContextUsage({context_window:{current_usage:null}}), null));
check(() => assert.equal(parseContextUsage({context_window:{context_window_size:null,current_usage:{input_tokens:1}}}), null));
check(() => assert.deepEqual(parseContextUsage({type:"assistant",message:{usage:{input_tokens:7}}}), {
  used_tokens:7,window_tokens:0,cost_usd:null,source:"claude-jsonl",
}));
const user = {statusLine:{type:"command",command:"cat",padding:2},hooks:{opaque:[]}};
const original = JSON.stringify(user);
const settings = JSON.parse(statuslineSettings(user)!);
check(() => assert.equal(JSON.stringify(user), original));
check(() => assert.deepEqual(Object.keys(settings), ["statusLine"]));
check(() => assert.equal(settings.statusLine.padding, 2));
const env = {...process.env};
delete env.CODETERM_API_URL;
delete env.CODETERM_API_TOKEN;
delete env.CODETERM_TAB_ID;
const payload = JSON.stringify(record);
const gitPath = process.platform === "win32" ? spawnSync("git", ["--exec-path"], {encoding:"utf8"}).stdout.trim() : "";
const bash = process.platform === "win32" ? path.resolve(gitPath, "../../../bin/bash.exe") : "bash";
const chained = spawnSync(bash, ["-c", settings.statusLine.command], {input:payload,encoding:"utf8",env});
check(() => assert.equal(chained.status, 0, chained.stderr));
check(() => assert.equal(chained.stdout, payload));
const failedForward = spawnSync(bash, ["-c", `curl() { cat >/dev/null; return 7; }; ${settings.statusLine.command}`], {
  input:payload,encoding:"utf8",env:{...env,CODETERM_API_URL:"fixture",CODETERM_API_TOKEN:"fixture",CODETERM_TAB_ID:"fixture"},
});
check(() => assert.equal(failedForward.status, 0, failedForward.stderr));
check(() => assert.equal(failedForward.stdout, payload));
check(() => assert.equal(statuslineSettings({statusLine:false}), null));
check(() => assert.equal(statuslineSettings({statusLine:{type:"command",command:"x".repeat(8193)}}), null));
check(() => assert.equal(statuslineSettings({statusLine:{type:"command",command:"cat",padding:{opaque:true}}}), null));
check(() => assert.deepEqual(statuslineLaunchArgs(["--model","opaque"]), ["--model","opaque"]));
check(() => assert.deepEqual(statuslineLaunchArgs([STATUSLINE_FLAG,"--settings","opaque"]), ["--settings","opaque"]));
let reads = 0;
(globalThis as any).host = {
  expandHome: () => "/fixture/settings.json",
  fs: {fileExists: () => true,readFileHead: () => { reads++; return original; }},
};
const args = statuslineLaunchArgs([STATUSLINE_FLAG,"--model","opaque"]);
check(() => assert.equal(reads, 1));
check(() => assert.equal(args[2], "--settings"));
check(() => assert.deepEqual(JSON.parse(args[3]), settings));
(globalThis as any).host.fs.readFileHead = () => "x".repeat(65_537);
check(() => assert.deepEqual(statuslineLaunchArgs([STATUSLINE_FLAG]), []));
const source = fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8");
const sandbox = {module:{exports:{} as any},exports:{},host:{}};
vm.runInNewContext(source, sandbox);
const plugin = sandbox.module.exports.default;
check(() => assert.deepEqual(JSON.parse(JSON.stringify(plugin.parseContextUsage(record))), parseContextUsage(record)));
const delta = plugin.parseSessionDelta(JSON.stringify({type:"assistant",uuid:"own",message:{content:[{type:"text",text:"opaque"}],usage:{input_tokens:9,cache_read_input_tokens:2}}}));
check(() => assert.equal(delta.messages[0].context_usage.used_tokens, 11));
for (const platform of ["linux", "windows"]) {
  Object.assign(sandbox.host, {
    platform: () => platform,
    shell: {quoteFor: (value: string) => `'${value.replace(/'/g, "'\\''")}'`},
  });
  const settingsArg = '{"opaque":"value with spaces"}';
  const command = plugin.buildLaunchCommand({args:["--settings", settingsArg]});
  check(() => assert.ok(command.includes("claude --settings ")));
  check(() => assert.ok(command.includes(`'${settingsArg}'`)));
}
console.log(`${checks} passed, 0 failed`);
