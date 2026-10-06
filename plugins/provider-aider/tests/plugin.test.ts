/**
 * Aider plugin contract + detection tests.
 * Run: npx tsx plugins/provider-aider/tests/plugin.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";
import runtime, {
  extractBannerModel,
  lastNonEmptyLine,
  MODE_PROMPT_RE,
  promptSafeChoice,
} from "../src/plugin";
import { parseAiderHistory } from "../src/history";

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

console.log("\n--- meta ---\n");
eq(plugin.meta.id, "aider", "meta.id is 'aider'");
eq(plugin.meta.displayName, "Aider", "meta.displayName");

console.log("\n--- detect ---\n");
assert(plugin.detect({ bytes: "", title: "aider" }), "title 'aider' positive");
assert(plugin.detect({ bytes: "Aider v0.82" }), "PTY 'Aider' substring positive");
assert(!plugin.detect({ bytes: "$ ls\n", title: "bash" }), "plain shell negative");
assert(!plugin.detect({ bytes: "", title: "opencode" }), "opencode title is not aider");

console.log("\n--- parseLine + statusOf ---\n");
eq(plugin.parseLine('{"x":1}'), [], "parseLine returns empty");
eq(plugin.statusOf({ kind: "anything" }), null, "statusOf always returns null");

console.log("\n--- titlePattern + noisePatterns ---\n");
assert(plugin.titlePattern instanceof RegExp, "titlePattern is RegExp");
assert(plugin.titlePattern!.test("aider"), "titlePattern matches 'aider'");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns non-empty");

console.log("\n--- launch args ---\n");
function loadAiderWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const aiderHost = (over: Record<string, unknown>) => ({
  platform: () => "linux",
  shell: { quoteFor: (v: unknown) => `'${String(v)}'`, quote: (v: unknown) => `'${String(v)}'` },
  fs: { writeTempFile: () => "/tmp/task.md", fileExists: () => false },
  log: () => {},
  ...over,
});

{
  const p = loadAiderWithHost(aiderHost({}));
  const launch = p.buildLaunchCommand({ task: "do the thing", starterPrompt: "assigned_task" });
  assert(launch.startsWith("aider "), "launch starts with aider");
  assert(launch.includes("--no-auto-commits"), "launch disables auto commits");
  assert(launch.includes("--no-pretty"), "launch disables pretty output");
  assert(launch.includes("--no-fancy-input"), "launch disables fancy input");
  assert(launch.includes("--chat-language English"), "launch sets chat language");
  assert(!launch.includes("sk-"), "launch carries no secrets");
  assert(!launch.includes("--config"), "no config flag without a config path");

  const withConfig = p.buildLaunchCommand({ configPath: "/home/u/.aider.conf.yml" });
  assert(withConfig.includes("--config '/home/u/.aider.conf.yml'"), "config flag present when set");
}

console.log("\n--- idle / busy detection ---\n");
{
  const idleScreen = [
    "Aider v0.82",
    "Model: openai/gpt-5.4-mini",
    "",
    "Tokens: 1.2k sent, 3.4k received.",
    "",
    ">",
  ].join("\n");
  const busyScreen = [
    "Aider v0.82",
    "Model: openai/gpt-5.4-mini",
    "",
    "Thinking...",
  ].join("\n");
  assert(MODE_PROMPT_RE.test(lastNonEmptyLine(idleScreen)), "idle screen ends with bare prompt");
  assert(MODE_PROMPT_RE.test(lastNonEmptyLine("ask>")), "mode-prefixed prompt is idle");
  assert(!MODE_PROMPT_RE.test(lastNonEmptyLine(busyScreen)), "busy screen does not end with prompt");
}

console.log("\n--- banner model parse ---\n");
{
  eq(extractBannerModel("Model: openai/gpt-5.4-mini\n"), "openai/gpt-5.4-mini", "banner model parsed");
  eq(extractBannerModel("no model here"), null, "no banner model → null");
}

console.log("\n--- shell-command prompt classification ---\n");
{
  const safeScreen = [
    "Run shell command? (Y)es/(N)o",
    "$ codeterm report done",
  ].join("\n");
  const unsafeScreen = [
    "Run shell command? (Y)es/(N)o",
    "$ rm -rf /",
  ].join("\n");
  const mixedScreen = [
    "Run shell commands? (Y)es/(N)o",
    "$ codeterm report ok",
    "$ ls -la",
  ].join("\n");
  const safePrompt = runtime.parsePrompt(safeScreen)!;
  eq(promptSafeChoice(safePrompt, safeScreen), "1", "codeterm-only command is safe yes");
  const unsafePrompt = runtime.parsePrompt(unsafeScreen)!;
  eq(promptSafeChoice(unsafePrompt, unsafeScreen), null, "non-codeterm command is not safe");
  const mixedPrompt = runtime.parsePrompt(mixedScreen)!;
  eq(promptSafeChoice(mixedPrompt, mixedScreen), null, "mixed command list is not safe");
}

console.log("\n--- owner prompts ---\n");
{
  const addFile = runtime.parsePrompt("Add file to the chat? (Y)es/(N)o");
  assert(!!addFile, "add-file prompt is parsed");
  eq(promptSafeChoice(addFile!, "Add file to the chat? (Y)es/(N)o"), null, "add-file prompt is not auto-answered");
  const createFile = runtime.parsePrompt("Create new file src/x.ts? (Y)es/(N)o");
  assert(!!createFile, "create-file prompt is parsed");
  eq(promptSafeChoice(createFile!, "Create new file src/x.ts? (Y)es/(N)o"), null, "create-file prompt is not auto-answered");
}

console.log("\n--- gitignore startup prompt ---\n");
{
  const startupScreen = [
    "You can skip this check with --no-gitignore",
    "Add .aider* to .gitignore (recommended)? (Y)es/(N)o [Yes]:",
  ].join("\n");
  const gitignorePrompt = runtime.parsePrompt(startupScreen);
  assert(!!gitignorePrompt, "gitignore startup prompt is parsed");
  eq(gitignorePrompt!.question, "Add .aider* to .gitignore (recommended)? (Y)es/(N)o [Yes]:", "question text captured");
  eq(promptSafeChoice(gitignorePrompt!, startupScreen), "1", "gitignore prompt is safe yes");
  const noDefault = runtime.parsePrompt("Add .aider* to .gitignore (recommended)? (Y)es/(N)o");
  assert(!!noDefault, "gitignore prompt without default is parsed");
  eq(promptSafeChoice(noDefault!, "Add .aider* to .gitignore (recommended)? (Y)es/(N)o"), "1", "gitignore prompt without default is safe yes");
}

console.log("\n--- session/model honesty ---\n");
{
  eq(runtime.detectSessionId("/cwd", [], 300000), null, "detectSessionId is null");
  eq(runtime.detectSessionModel("/cwd", "ses_x"), null, "detectSessionModel is null");
  eq(runtime.sessionExists("/cwd", "ses_x"), false, "sessionExists is false");
  eq(runtime.enumerateSessions(), [], "enumerateSessions is empty");
  eq(runtime.findSession("ses_x"), null, "findSession is null");
  eq(runtime.usageSources("/cwd", "ses_x"), [], "usageSources is empty");
}

console.log("\n--- history parser ---\n");
{
  const singleTurn = [
    "#### Hello, what can you do?",
    "",
    "I can help you with coding tasks, answer questions, and assist with various development activities.",
  ].join("\n");
  const result1 = parseAiderHistory(singleTurn, 0);
  eq(result1.messages.length, 2, "single turn has 2 messages");
  eq(result1.messages[0].role, "user", "first message is user");
  eq(result1.messages[0].text, "Hello, what can you do?", "user text parsed");
  eq(result1.messages[1].role, "assistant", "second message is assistant");
  eq(result1.messages[1].text, "I can help you with coding tasks, answer questions, and assist with various development activities.", "assistant text parsed");

  const multiTurn = [
    "#### First question",
    "",
    "First answer",
    "",
    "#### Second question",
    "",
    "Second answer with",
    "multiple lines",
  ].join("\n");
  const result2 = parseAiderHistory(multiTurn, 0);
  eq(result2.messages.length, 4, "multi-turn has 4 messages");
  eq(result2.messages[0].role, "user", "turn 1 user");
  eq(result2.messages[1].role, "assistant", "turn 1 assistant");
  eq(result2.messages[2].role, "user", "turn 2 user");
  eq(result2.messages[3].role, "assistant", "turn 2 assistant");
  eq(result2.messages[3].text, "Second answer with\nmultiple lines", "multi-line assistant text");

  const emptyResult = parseAiderHistory("", 0);
  eq(emptyResult.messages.length, 0, "empty content has no messages");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
