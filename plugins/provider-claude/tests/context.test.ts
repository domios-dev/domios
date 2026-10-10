import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { parseContextUsage, composeSettingsArgs, statuslineSettings, STATUSLINE_FLAG } from "../src/context";

let checks = 0;
function check(run: () => void) { run(); checks++; }
function settingsPath(command: string): string {
  const flag = "--settings ";
  const offset = command.indexOf(flag);
  assert.ok(offset >= 0);
  const token = command.slice(offset + flag.length);
  return token.startsWith("'") && token.endsWith("'")
    ? token.slice(1, -1).replaceAll("'\\''", "'")
    : token;
}
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
const compact = {type:"system", subtype:"compact_boundary", uuid:"compact-own",
  compactMetadata:{preTokens:170000,postTokens:25000}};
const compactContext = {used_tokens:25000,window_tokens:0,cost_usd:null,source:"claude-jsonl-compaction"};
check(() => assert.deepEqual(parseContextUsage(compact), compactContext));
check(() => assert.equal(parseContextUsage({...compact,subtype:"other"}), null));
check(() => assert.equal(parseContextUsage({...compact,type:"user"}), null));
check(() => assert.equal(parseContextUsage({...compact,compactMetadata:{preTokens:170000}}), null));
for (const postTokens of [null,-1,0.5,"25000",10_000_001]) {
  check(() => assert.equal(parseContextUsage({...compact,compactMetadata:{postTokens}}), null));
}
check(() => assert.equal(parseContextUsage({...compact,compactMetadata:{postTokens:0}})?.used_tokens, 0));
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
check(() => assert.deepEqual(composeSettingsArgs(["--model","opaque"]), ["--model","opaque","--settings",'{"showThinkingSummaries":true}']));
let reads = 0;
(globalThis as any).host = {
  expandHome: () => "/fixture/settings.json",
  fs: {fileExists: () => true,readFileHead: () => { reads++; return original; }},
};
const args = composeSettingsArgs([STATUSLINE_FLAG,"--model","opaque"])!;
check(() => assert.equal(reads, 1));
check(() => assert.equal(args[2], "--settings"));
check(() => assert.deepEqual(JSON.parse(args[3]), {...settings,showThinkingSummaries:true}));
(globalThis as any).host.fs.readFileHead = () => "x".repeat(65_537);
check(() => assert.deepEqual(composeSettingsArgs([STATUSLINE_FLAG]), ["--settings",'{"showThinkingSummaries":true}']));
check(() => assert.equal(composeSettingsArgs(["--settings","opaque"]), null));
const source = fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8");
let writes = 0;
const launchFiles = new Map<string, string>();
const readonlyHost = {
  platform: () => "linux", log: () => {}, expandHome: () => "/fixture/settings.json",
  shell: {quoteFor: (value: string) => value},
  fs: {fileExists: () => true, readFileHead: () => original,
    writeTempFile: (body: string, suffix: string) => {
      const path = `/fixture/runtime/task files/launch-${launchFiles.size}${suffix}`;
      launchFiles.set(path, body);
      return path;
    },
    writeFile: () => { writes++; throw new Error("unexpected settings write"); },
    writeJson: () => { writes++; throw new Error("unexpected settings write"); }},
};
const sandbox = {module:{exports:{} as any},exports:{},host:readonlyHost};
vm.runInNewContext(source, sandbox);
const plugin = sandbox.module.exports.default;
check(() => assert.deepEqual(JSON.parse(JSON.stringify(plugin.parseContextUsage(record))), parseContextUsage(record)));
const delta = plugin.parseSessionDelta(JSON.stringify({type:"assistant",uuid:"own",message:{content:[{type:"text",text:"opaque"}],usage:{input_tokens:9,cache_read_input_tokens:2}}}));
check(() => assert.equal(delta.messages[0].context_usage.used_tokens, 11));
const compactDelta = plugin.parseSessionDelta(JSON.stringify(compact));
check(() => assert.equal(compactDelta.messages.length, 1));
check(() => assert.deepEqual(JSON.parse(JSON.stringify(compactDelta.messages[0].context_usage)), compactContext));
check(() => assert.equal(compactDelta.messages[0].timelineMarker.kind, "context_compaction"));
for (const command of [
  plugin.buildLaunchCommand({}),
  plugin.buildResumeCommand("own", false),
  plugin.buildResumeCommandWithContext({sessionId:"own",args:[]}),
  plugin.buildLaunchCommand({args:["--settings",original]}),
  plugin.buildResumeCommandWithContext({sessionId:"own",args:["--settings=/fixture/extra.json"]}),
]) {
  check(() => assert.equal(command.split("--settings").length, 2));
  const path = settingsPath(command);
  check(() => assert.ok(path.endsWith(".ct-settings.json")));
  check(() => assert.equal(command.includes("showThinkingSummaries"), false));
  const overlay = JSON.parse(launchFiles.get(path)!);
  check(() => assert.equal(overlay.showThinkingSummaries, true));
}
const mergedCommand = plugin.buildLaunchCommand({args:["--settings",original]});
const mergedSettings = JSON.parse(launchFiles.get(settingsPath(mergedCommand))!);
check(() => assert.deepEqual(mergedSettings.hooks, user.hooks));
check(() => assert.deepEqual(mergedSettings.statusLine, user.statusLine));
check(() => assert.equal(writes, 0));
check(() => assert.equal(JSON.stringify(user), original));
for (const platform of ["linux", "windows"]) {
  Object.assign(sandbox.host, {
    platform: () => platform,
    shell: {quoteFor: (value: string) => `'${value.replace(/'/g, "'\\''")}'`},
  });
  const settingsArg = '{"opaque":"value with spaces"}';
  const command = plugin.buildLaunchCommand({args:["--settings", settingsArg]});
  check(() => assert.ok(command.includes("claude --settings ")));
  const path = settingsPath(command);
  check(() => assert.ok(command.includes(`'${path}'`)));
  check(() => assert.equal(command.includes("value with spaces"), false));
  check(() => assert.equal(JSON.parse(launchFiles.get(path)!).opaque, "value with spaces"));
}
sandbox.host.fs.writeTempFile = () => null as any;
check(() => assert.equal(plugin.buildLaunchCommand({}), ""));
check(() => assert.equal(plugin.buildResumeCommand("own", false), null));
check(() => assert.equal(plugin.buildResumeCommandWithContext({sessionId:"own",args:[]}), null));
console.log(`${checks} passed, 0 failed`);
