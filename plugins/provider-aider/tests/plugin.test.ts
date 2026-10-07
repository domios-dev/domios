/**
 * Aider plugin contract + detection tests.
 * Run: npx tsx plugins/provider-aider/tests/plugin.test.ts
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";
import runtime, {
  deliverText,
  extractBannerModel,
  lastNonEmptyLine,
  MODE_PROMPT_RE,
  pickTaggedBlockTag,
  promptSafeChoice,
  wrapMultiline,
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
  assert(launch.includes("--no-show-model-warnings"), "launch suppresses model warnings");

  const secretHost = aiderHost({
    secretGet: (name: string) => (name === "my-key" ? "sk-secret-value" : null),
  });
  const withSecret = loadAiderWithHost(secretHost).buildLaunchCommand({
    apiBase: "https://api.example.com/v1",
    apiKeySecret: "my-key",
  });
  assert(withSecret.includes("OPENAI_API_BASE='https://api.example.com/v1'"), "api base exported");
  assert(withSecret.includes("codeterm mem secret get --name 'my-key'"), "api key fetched at run time");
  assert(!withSecret.includes("sk-secret-value"), "secret value never appears in the command");
  assert(!withSecret.includes("--api-key"), "secret not in argv");
}

console.log("\n--- secret-safe launch command ---\n");
{
  const secretHost = aiderHost({
    secretGet: (name: string) => (name === "my-key" ? "sk-secret-value" : null),
  });
  const p = loadAiderWithHost(secretHost);
  const posix = p.buildLaunchCommand({ apiKeySecret: "my-key" });
  assert(posix.includes('export OPENAI_API_KEY="$(codeterm mem secret get --name \'my-key\')"'), "posix shell fetches the key at run time");
  assert(!posix.includes("sk-secret-value"), "posix command carries no secret value");
  const winHost = aiderHost({
    platform: () => "windows",
    shell: {
      quoteFor: (v: unknown) => `"${String(v)}"`,
      quote: (v: unknown) => `"${String(v)}"`,
    },
    secretGet: (name: string) => (name === "my-key" ? "sk-secret-value" : null),
  });
  const win = loadAiderWithHost(winHost).buildLaunchCommand({ apiKeySecret: "my-key" });
  assert(win.includes('$env:OPENAI_API_KEY=(codeterm mem secret get --name "my-key")'), "powershell fetches the key at run time");
  assert(!win.includes("sk-secret-value"), "powershell command carries no secret value");
}

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

console.log("\n--- idle / busy detection ---\n");
{
  assert(MODE_PROMPT_RE.test(lastNonEmptyLine(idleScreen)), "idle screen ends with bare prompt");
  assert(MODE_PROMPT_RE.test(lastNonEmptyLine("ask>")), "mode-prefixed prompt is idle");
  assert(!MODE_PROMPT_RE.test(lastNonEmptyLine(busyScreen)), "busy screen does not end with prompt");
}

console.log("\n--- structural composer readiness ---\n");
{
  assert(runtime.screenHasTui(idleScreen), "idle aider screen with composer prompt is ready");
  assert(runtime.screenHasTui("ask>"), "mode-prefixed composer prompt is ready");
  assert(!runtime.screenHasTui(busyScreen), "busy screen is not ready");
  assert(!runtime.screenHasTui("aider"), "banner without composer prompt is not ready");
  assert(!runtime.screenHasTui(">"), "bare prompt without aider signal is not ready");
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

console.log("\n--- docs url startup prompt ---\n");
{
  const docsScreen = [
    "Warning: openai/mimo-v2.6-pro expects these environment variables - OPENAI_API_KEY: Not set",
    "Open documentation url for more info? (Y)es/(N)o/(D)on't ask again [Yes]:",
  ].join("\n");
  eq(runtime.parsePrompt(docsScreen), null, "docs url prompt is not an owner prompt");
  eq(runtime.launchOnboardingSafeChoice!(docsScreen), "3", "docs url prompt is safe don't-ask-again");
  const noDefault = "Open documentation url for more info? (Y)es/(N)o/(D)on't ask again";
  eq(runtime.parsePrompt(noDefault), null, "docs url prompt without default is not an owner prompt");
  eq(runtime.launchOnboardingSafeChoice!(noDefault), "3", "docs url prompt without default is safe don't-ask-again");
}

console.log("\n--- gitignore startup prompt ---\n");
{
  const startupScreen = [
    "You can skip this check with --no-gitignore",
    "Add .aider* to .gitignore (recommended)? (Y)es/(N)o [Yes]:",
  ].join("\n");
  eq(runtime.parsePrompt(startupScreen), null, "gitignore startup prompt is not an owner prompt");
  const onboarding = runtime.launchOnboardingResponse!(startupScreen);
  assert(!!onboarding, "gitignore startup prompt is a launch onboarding step");
  eq(onboarding!.step, "WQ==", "gitignore onboarding step answers yes");
  eq(runtime.launchOnboardingSafeChoice!(startupScreen), "1", "gitignore safe choice is yes");
  const noDefault = "Add .aider* to .gitignore (recommended)? (Y)es/(N)o";
  eq(runtime.parsePrompt(noDefault), null, "gitignore prompt without default is not an owner prompt");
  assert(!!runtime.launchOnboardingResponse!(noDefault), "gitignore prompt without default is a launch onboarding step");
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

  const blankTurns = [
    "#### ",
    "",
    "#### Real question",
    "",
    "Real answer",
  ].join("\n");
  const blankResult = parseAiderHistory(blankTurns, 0);
  eq(blankResult.messages.length, 2, "blank #### turns are skipped");
  eq(blankResult.messages[0].role, "user", "blank turn does not become a user row");
  eq(blankResult.messages[0].text, "Real question", "real user turn survives");
  eq(blankResult.messages[1].role, "assistant", "assistant turn survives");
}

console.log("\n--- history session blocks ---\n");
{
  const twoSessions = [
    "# aider chat started at 2026-10-06 10:00:00.000",
    "",
    "#### Old question",
    "",
    "Old answer",
    "",
    "# aider chat started at 2026-10-06 12:00:00.000",
    "",
    "#### New question",
    "",
    "New answer",
  ].join("\n");
  const launchMs = Date.parse("2026-10-06 11:00:00.000");
  const result = parseAiderHistory(twoSessions, launchMs);
  eq(result.messages.length, 2, "only the current session block is returned");
  eq(result.messages[0].text, "New question", "old session user turn is excluded");
  eq(result.messages[1].text, "New answer", "old session assistant turn is excluded");
  eq(result.messages[0].ts, Date.parse("2026-10-06 12:00:00.000"), "message carries its session start");

  const all = parseAiderHistory(twoSessions, 0);
  eq(all.messages.length, 4, "sinceMs 0 keeps every session block");
  eq(all.messages[0].text, "Old question", "first session kept when unfiltered");

  const localHeader = [
    "# aider chat started at 2026-10-06 12:00:00.000",
    "",
    "#### Local question",
    "",
    "Local answer",
  ].join("\n");
  const localLaunch = new Date(2026, 9, 6, 11, 0, 0, 0).getTime();
  const localResult = parseAiderHistory(localHeader, localLaunch);
  eq(localResult.messages.length, 2, "local-time header block is accepted at local launch time");
  eq(localResult.messages[0].text, "Local question", "local-time user turn parsed");
  const localBefore = new Date(2026, 9, 6, 13, 0, 0, 0).getTime();
  const localExcluded = parseAiderHistory(localHeader, localBefore);
  eq(localExcluded.messages.length, 0, "local-time header block is excluded when launch is later");
}

console.log("\n--- history file reader fallbacks ---\n");
{
  const tmp = path.join(os.tmpdir(), `aider-history-${Date.now()}.md`);
  fs.writeFileSync(tmp, "#### Hi\n\nHello there\n", "utf8");
  const readVia = (host: any) => {
    const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
    const source = fs.readFileSync(pluginPath, "utf8");
    const context = { host, console, module: { exports: {} as any }, exports: {} };
    new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
    return context.module.exports.default;
  };
  const bare = readVia({ platform: () => "linux", shell: { quoteFor: (v: any) => `'${v}'` } });
  const bareResult = bare.readStructuredChat(path.dirname(tmp), "ses_x");
  eq(bareResult.messages.length, 2, "host without fs still reads the history file");
  eq(bareResult.messages[0].content, "Hi", "host without fs parses the user turn");
  const viaHostReadFile = readVia({
    platform: () => "linux",
    shell: { quoteFor: (v: any) => `'${v}'` },
    readFile: (p: string) => fs.readFileSync(p, "utf8"),
  });
  eq(viaHostReadFile.readStructuredChat(path.dirname(tmp), "ses_x").messages.length, 2, "host.readFile is used when present");
  const viaFsReadFile = readVia({
    platform: () => "linux",
    shell: { quoteFor: (v: any) => `'${v}'` },
    fs: { readFile: (p: string) => fs.readFileSync(p, "utf8") },
  });
  eq(viaFsReadFile.readStructuredChat(path.dirname(tmp), "ses_x").messages.length, 2, "host.fs.readFile is used when present");
  fs.unlinkSync(tmp);
}

console.log("\n--- multi-line chat delivery ---\n");
{
  eq(deliverText("hello"), "hello", "single-line text is untouched");
  eq(deliverText("hello\nworld"), "{domios\nhello\nworld\ndomios}", "multi-line text becomes one block");
  eq(deliverText("a\nb\n\nc\nd"), "{domios\na\nb\n\nc\nd\ndomios}", "blank lines stay inside one block");
  eq(deliverText("x\ny"), wrapMultiline("x\ny"), "wrapMultiline matches deliverText for multi-line input");
  const long = Array.from({ length: 300 }, (_, i) => `line ${i}`).join("\n");
  const wrapped = deliverText(long);
  assert(wrapped.startsWith("{domios\n"), "long multi-line text starts with one block");
  assert(wrapped.endsWith("domios}"), "long multi-line text ends with one block");
  const blocks = wrapped.match(/^\{[a-z0-9]+\n[\s\S]*\n[a-z0-9]+\}$/g) || [];
  eq(blocks.length, 1, "one send is exactly one block");
  assert(wrapped.includes("line 0\n") && wrapped.includes("line 299"), "the whole payload stays inside the block");
}

console.log("\n--- tagged block tag selection ---\n");
{
  eq(pickTaggedBlockTag("plain text"), "domios", "default tag when text carries none");
  eq(pickTaggedBlockTag("has {domios} inside"), "domios1", "tag avoids an opening marker in the text");
  eq(pickTaggedBlockTag("has {/domios} inside"), "domios1", "tag avoids a closing marker in the text");
  eq(pickTaggedBlockTag("{domios} and {domios1}"), "domios2", "tag skips every colliding candidate");
  const collision = deliverText("body {domios} tail\nmore");
  assert(collision.startsWith("{domios1\n"), "colliding text uses the next free tag");
  assert(collision.endsWith("domios1}"), "colliding text closes with the same tag");
  assert(collision.includes("body {domios} tail"), "the payload is never rewritten");
}

console.log("\n--- chatPreprocess provider scope ---\n");
{
  eq(runtime.chatPreprocess!({ provider: "claude", text: "a\nb" }), null, "other providers are untouched");
  eq(runtime.chatPreprocess!({ provider: "codex", text: "a\nb" }), null, "codex is untouched");
  eq(runtime.chatPreprocess!({ provider: "aider", text: "a\nb" }), { text: "{domios\na\nb\ndomios}" }, "aider text is wrapped");
  eq(runtime.chatPreprocess!({ provider: "aider", text: "single" }), null, "single-line aider text passes through");
  eq(runtime.chatPreprocess!({ provider: null, text: "a\nb" }), null, "missing provider is untouched");
}

console.log("\n--- launch onboarding auto-answer ---\n");
{
  const gitignoreScreen = [
    "You can skip this check with --no-gitignore",
    "Add .aider* to .gitignore (recommended)? (Y)es/(N)o [Yes]:",
  ].join("\n");
  const response = runtime.launchOnboardingResponse!(gitignoreScreen);
  assert(!!response, "gitignore prompt is a launch onboarding step");
  eq(response!.step, "WQ==", "gitignore onboarding step answers yes");
  eq(runtime.launchOnboardingSafeChoice!(gitignoreScreen), "1", "gitignore safe choice is yes");
  const docsScreen = [
    "Warning: openai/mimo-v2.6-pro expects these environment variables - OPENAI_API_KEY: Not set",
    "Open documentation url for more info? (Y)es/(N)o/(D)on't ask again [Yes]:",
  ].join("\n");
  eq(runtime.launchOnboardingSafeChoice!(docsScreen), "3", "docs url safe choice is don't-ask-again");
  eq(runtime.launchOnboardingResponse!("no prompt here"), null, "no prompt yields no onboarding step");
  eq(runtime.parsePrompt(gitignoreScreen), null, "parsePrompt leaves the gitignore startup dialog to onboarding");
  assert(!!runtime.launchOnboardingResponse!(gitignoreScreen), "launchOnboardingResponse still answers the gitignore startup dialog");
  const trailingSpaceScreen = [
    "You can skip this check with --no-gitignore",
    "Add .aider* to .gitignore (recommended)? (Y)es/(N)o [Yes]: ",
  ].join("\n");
  eq(runtime.parsePrompt(trailingSpaceScreen), null, "trailing-space gitignore line is not an owner prompt");
  assert(!!runtime.launchOnboardingResponse!(trailingSpaceScreen), "trailing-space gitignore line is still a launch onboarding step");
}

console.log("\n--- manifest paste and preprocessor capability ---\n");
{
  const manifest = JSON.parse(
    fs.readFileSync(
      path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"),
      "utf8",
    ),
  );
  eq(manifest.capabilities?.chatPreprocessor?.match, "text", "manifest declares the chatPreprocessor capability");
  eq(manifest.capabilities?.agentProvider, true, "manifest declares the agentProvider capability");
  eq(manifest.spawn?.inputFallbackMs, 5000, "manifest declares the bounded input fallback");
  eq(manifest.settingsSchema, "settings.schema.json", "manifest declares the settings schema file");
  const schema = JSON.parse(
    fs.readFileSync(
      path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "settings.schema.json"),
      "utf8",
    ),
  );
  const keys = schema.flatMap((section: any) => (section.fields || []).map((f: any) => f.key));
  eq(keys, ["configPath", "apiBase", "apiKeySecret"], "settings schema declares the three launch settings");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
