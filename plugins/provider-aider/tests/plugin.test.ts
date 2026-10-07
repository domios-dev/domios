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
import { parseAiderHistory, parseAiderHistoryDelta } from "../src/history";

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
  assert(launch.includes("aider "), "launch invokes aider");
  assert(launch.includes("mkdir -p "), "launch creates the history folder");
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

console.log("\n--- settings-driven launch command ---\n");
{
  const settingsHost = aiderHost({
    settingsJson: () =>
      JSON.stringify({
        apiKeySecret: "my-key",
        apiBase: "https://api.example.com/v1",
        configPath: "/home/u/.aider.conf.yml",
      }),
    secretGet: (name: string) => (name === "my-key" ? "sk-secret-value" : null),
  });
  const p = loadAiderWithHost(settingsHost);
  const launch = p.buildLaunchCommand({});
  assert(launch.includes("OPENAI_API_BASE='https://api.example.com/v1'"), "api base comes from host settings");
  assert(launch.includes('export OPENAI_API_KEY="$(codeterm mem secret get --name \'my-key\')"'), "api key name comes from host settings");
  assert(launch.includes("--config '/home/u/.aider.conf.yml'"), "config path comes from host settings");
  assert(!launch.includes("sk-secret-value"), "secret value never appears in the settings-driven command");
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
  eq(runtime.findSession("ses_x"), null, "findSession is null for unknown id");
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
    const context = {
      host,
      console,
      module: { exports: {} as any },
      exports: {},
    };
    new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
    return context.module.exports.default;
  };
  const historyPath = `${path.dirname(tmp)}/.aider.chat.history.md`;
  const viaFsReadFile = readVia({
    platform: () => "linux",
    shell: { quoteFor: (v: any) => `'${v}'` },
    fs: {
      readFile: (p: string) =>
        p === historyPath ? fs.readFileSync(tmp, "utf8") : null,
    },
  });
  eq(viaFsReadFile.sessionJsonlPath(path.dirname(tmp), "ses_x"), `${path.dirname(tmp)}/.aider/history/ses_x.md`, "sessionJsonlPath names the per-nonce history file");
  const delta = viaFsReadFile.parseSessionDelta(fs.readFileSync(tmp, "utf8")) as {
    messages: Array<{ role: string; blocks: Array<{ kind: string; data: { text: string } }> }>;
  };
  eq(delta.messages.length, 2, "parseSessionDelta turns the file tail into rows");
  eq(delta.messages[0].role, "user", "delta user row");
  eq(delta.messages[0].blocks[0].kind, "text", "delta user block kind");
  eq(delta.messages[0].blocks[0].data.text, "Hi", "delta user text");
  eq(delta.messages[1].role, "assistant", "delta assistant row");
  eq(delta.messages[1].blocks[0].kind, "text", "delta assistant block kind");
  eq(delta.messages[1].blocks[0].data.text, "Hello there", "delta assistant text");
  const missingFs = readVia({ platform: () => "linux", shell: { quoteFor: (v: any) => `'${v}'` } });
  eq(missingFs.sessionJsonlPath(path.dirname(tmp), "ses_x"), `${path.dirname(tmp)}/.aider/history/ses_x.md`, "sessionJsonlPath needs no fs");
  eq((missingFs.parseSessionDelta("") as { messages: unknown[] }).messages.length, 0, "empty delta yields no rows");
  fs.unlinkSync(tmp);
}

console.log("\n--- history cursor advances ---\n");
{
  const content = [
    "#### Q1",
    "",
    "A1",
    "",
    "#### Q2",
    "",
    "A2",
  ].join("\n");
  const first = parseAiderHistory(content, 0);
  eq(first.messages.length, 4, "first read returns all 4 messages");
  eq(first.cursor, "4", "cursor is the message count");
  const second = parseAiderHistory(content, 0, first.cursor!);
  eq(second.messages.length, 4, "second read still returns all messages (host merges by id)");
  eq(second.cursor, "4", "cursor stays stable at the message count");
}

console.log("\n--- per-nonce session files ---\n");
{
  const readVia = (host: any) => {
    const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
    const source = fs.readFileSync(pluginPath, "utf8");
    const context = { host, console, module: { exports: {} as any }, exports: {} };
    new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
    return context.module.exports.default;
  };
  const now = Date.now();
  const historyDir = "/ws/.aider/history";
  const hostWith = (files: Array<{ name: string; modifiedMs: number | null }>) => ({
    platform: () => "linux",
    shell: { quoteFor: (v: any) => `'${v}'` },
    fs: {
      readFile: (p: string) => (p === `${historyDir}/nonce-b.md` ? "#### Q\n\nA\n" : null),
      readDir: (p: string) =>
        p === historyDir
          ? files.map((f) => ({
              name: f.name,
              path: `${historyDir}/${f.name}`,
              isFile: true,
              isDir: false,
              size: 10,
              modifiedMs: f.modifiedMs,
              createdMs: null,
            }))
          : [],
    },
    log: () => {},
  });
  const p = readVia(
    hostWith([
      { name: "nonce-a.md", modifiedMs: now - 5000 },
      { name: "nonce-b.md", modifiedMs: now - 1000 },
      { name: "nonce-a.input", modifiedMs: now - 5000 },
    ]),
  );
  eq(p.detectSessionId("/ws", [], 0), "nonce-b", "detectSessionId returns the newest history file");
  eq(p.detectSessionId("/ws", ["nonce-b"], 0), "nonce-a", "exclude list skips the named file");
  eq(p.detectSessionId("/ws", [], 2000), "nonce-b", "maxAgeMs keeps the fresh file");
  eq(p.sessionExists("/ws", "nonce-b"), true, "sessionExists finds a known nonce file");
  eq(p.sessionExists("/ws", "nonce-a"), true, "sessionExists finds the older nonce file");
  eq(p.sessionExists("/ws", "nope"), false, "sessionExists rejects an unknown id");
  eq(p.sessionJsonlPath("/ws", "nonce-b"), `${historyDir}/nonce-b.md`, "sessionJsonlPath names the nonce file");
  eq(p.sessionJsonlPath("", "nonce-b"), null, "sessionJsonlPath rejects empty cwd");
  eq(p.sessionJsonlPath("/ws", ""), null, "sessionJsonlPath rejects empty session id");
  eq(p.sessionFilePath("/ws", "nonce-b"), `${historyDir}/nonce-b.md`, "sessionFilePath names the nonce file");
  eq(p.sessionFilePath("", "nonce-b"), null, "sessionFilePath rejects empty cwd");
  eq(p.sessionFilePath("/ws", ""), null, "sessionFilePath rejects empty session id");
  const stale = readVia(hostWith([{ name: "nonce-a.md", modifiedMs: now - 7200000 }]));
  eq(stale.detectSessionId("/ws", [], 60000), null, "stale file is rejected within maxAgeMs");
  eq(stale.detectSessionId("/ws", [], 0), "nonce-a", "no maxAgeMs keeps the file regardless of age");
  eq(stale.detectSessionId("/ws", [], undefined), "nonce-a", "undefined maxAgeMs keeps the file");
  const unknownMtime = readVia(hostWith([{ name: "nonce-a.md", modifiedMs: null }]));
  eq(unknownMtime.detectSessionId("/ws", [], 60000), null, "unknown mtime is not fresh within maxAgeMs");
  eq(unknownMtime.detectSessionId("/ws", [], 0), "nonce-a", "unknown mtime keeps the file when no maxAgeMs is set");
  const empty = readVia(hostWith([]));
  eq(empty.detectSessionId("/ws", [], 0), null, "empty history dir yields no session");
  eq(
    p.detectLaunchSession!({ cwd: "/ws", launchMarker: "nonce-b", launchedAtMs: now }),
    { sessionId: "nonce-b", source: "launch_marker" },
    "detectLaunchSession binds the launch marker to its own history file",
  );
  eq(
    p.detectLaunchSession!({ cwd: "/ws", launchMarker: "nonce-z", launchedAtMs: now }),
    null,
    "detectLaunchSession rejects a marker with no history file",
  );
  eq(
    p.detectLaunchSession!({ cwd: "/ws", launchMarker: null, launchedAtMs: now }),
    null,
    "detectLaunchSession rejects a launch without a marker",
  );
}

console.log("\n--- thinking blocks stripped from assistant text ---\n");
{
  const withThinking = [
    "#### Q",
    "",
    "<thinking-content-abc>",
    "internal reasoning",
    "</thinking-content-abc>",
    "Final answer",
  ].join("\n");
  const result = parseAiderHistory(withThinking, 0);
  eq(result.messages.length, 2, "thinking block does not create extra rows");
  eq(result.messages[1].role, "assistant", "assistant row survives");
  eq(result.messages[1].text, "Final answer", "thinking block is stripped from assistant text");
  const onlyThinking = [
    "#### Q",
    "",
    "<thinking-content-abc>",
    "internal reasoning",
    "</thinking-content-abc>",
  ].join("\n");
  const empty = parseAiderHistory(onlyThinking, 0);
  eq(empty.messages.length, 1, "assistant row with only thinking content is dropped");
  eq(empty.messages[0].role, "user", "user row survives");
}

console.log("\n--- meta lines dropped from assistant text ---\n");
{
  const withMeta = [
    "#### Q",
    "",
    "> Tokens: 608 sent, 79 received.",
    "Final answer",
    "> Tokens: 700 sent, 90 received.",
  ].join("\n");
  const result = parseAiderHistory(withMeta, 0);
  eq(result.messages.length, 2, "meta lines do not create extra rows");
  eq(result.messages[1].role, "assistant", "assistant row survives meta lines");
  eq(result.messages[1].text, "Final answer", "meta lines are stripped from assistant text");
  const onlyMeta = [
    "#### Q",
    "",
    "> Tokens: 608 sent, 79 received.",
  ].join("\n");
  const empty = parseAiderHistory(onlyMeta, 0);
  eq(empty.messages.length, 1, "assistant row with only meta lines is dropped");
  eq(empty.messages[0].role, "user", "user row survives meta-only assistant");
  const delta = parseAiderHistoryDelta(withMeta) as {
    messages: Array<{ role: string; blocks: Array<{ kind: string; data: { text: string } }> }>;
  };
  eq(delta.messages.length, 2, "delta drops meta lines without extra rows");
  eq(delta.messages[1].blocks[0].data.text, "Final answer", "delta assistant text drops meta lines");
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

console.log("\n--- parseSessionDelta (file-tail chat route) ---\n");
{
  const chunk = [
    "#### Hello, what can you do?",
    "",
    "I can help you with coding tasks.",
    "",
    "#### Second question",
    "",
    "<thinking-content-abc>",
    "internal reasoning",
    "</thinking-content-abc>",
    "Second answer",
  ].join("\n");
  const delta = parseAiderHistoryDelta(chunk) as {
    messages: Array<{ role: string; blocks: Array<{ kind: string; data: { text: string } }>; recordIndex: number }>;
  };
  eq(delta.messages.length, 4, "delta yields user + assistant + user + assistant");
  eq(delta.messages[0].role, "user", "first delta row is user");
  eq(delta.messages[0].blocks[0].kind, "text", "first delta row is a text block");
  eq(delta.messages[0].blocks[0].data.text, "Hello, what can you do?", "first delta user text");
  eq(delta.messages[1].role, "assistant", "second delta row is assistant");
  eq(delta.messages[1].blocks[0].data.text, "I can help you with coding tasks.", "first delta assistant text");
  eq(delta.messages[2].role, "user", "third delta row is user");
  eq(delta.messages[2].blocks[0].data.text, "Second question", "second delta user text");
  eq(delta.messages[3].role, "assistant", "fourth delta row is assistant");
  eq(delta.messages[3].blocks[0].data.text, "Second answer", "thinking block stripped from delta assistant text");
  eq(delta.messages[0].recordIndex, 0, "recordIndex is the chunk line");
  eq(delta.messages[1].recordIndex, 2, "assistant recordIndex is its first line");

  const blankOnly = parseAiderHistoryDelta("#### \n\n#### Real\n\nAnswer\n") as {
    messages: Array<{ role: string; blocks: Array<{ kind: string; data: { text: string } }> }>;
  };
  eq(blankOnly.messages.length, 2, "blank #### turns are skipped in delta");
  eq(blankOnly.messages[0].blocks[0].data.text, "Real", "real user turn survives in delta");
  eq(blankOnly.messages[1].blocks[0].data.text, "Answer", "assistant turn survives in delta");

  const thinkingOnly = parseAiderHistoryDelta(
    "#### Q\n\n<thinking-content-x>\nhidden\n</thinking-content-x>\n",
  ) as { messages: Array<{ role: string }> };
  eq(thinkingOnly.messages.length, 1, "assistant row with only thinking is dropped in delta");
  eq(thinkingOnly.messages[0].role, "user", "user row survives in delta");

  const empty = parseAiderHistoryDelta("") as { messages: unknown[] };
  eq(empty.messages.length, 0, "empty chunk yields no rows");
}


console.log("\n--- structured chat exports removed ---\n");
{
  const p = loadAiderWithHost(aiderHost({}));
  eq(p.usesStructuredChat, undefined, "usesStructuredChat is removed");
  eq(p.readStructuredChat, undefined, "readStructuredChat is removed");
}

console.log("\n--- launch command carries per-nonce history files ---\n");
{
  const secretHost = aiderHost({
    secretGet: (name: string) => (name === "my-key" ? "sk-secret-value" : null),
  });
  const p = loadAiderWithHost(secretHost);
  const posix = p.buildLaunchCommand({ cwd: "/ws", sessionId: "nonce-x" });
  assert(posix.includes("mkdir -p '/ws/.aider/history'"), "posix launch creates the history folder");
  assert(
    posix.includes("export CODETERM_SESSION_BINDING_NONCE='nonce-x';"),
    "posix launch exports the binding nonce for the history flags",
  );
  assert(
    posix.includes('--chat-history-file "/ws/.aider/history/${CODETERM_SESSION_BINDING_NONCE}.md"'),
    "posix launch names the chat history file by the binding nonce",
  );
  assert(
    posix.includes('--input-history-file "/ws/.aider/history/${CODETERM_SESSION_BINDING_NONCE}.input"'),
    "posix launch names the input history file by the binding nonce",
  );
  assert(
    !posix.includes("'$CODETERM_SESSION_BINDING_NONCE'") &&
      !posix.includes("'$CODETERM_SESSION_BINDING_NONCE"),
    "posix nonce is never single-quoted",
  );
  const winHost = aiderHost({
    platform: () => "windows",
    shell: {
      quoteFor: (v: unknown) => `"${String(v)}"`,
      quote: (v: unknown) => `"${String(v)}"`,
    },
    secretGet: (name: string) => (name === "my-key" ? "sk-secret-value" : null),
  });
  const win = loadAiderWithHost(winHost).buildLaunchCommand({ cwd: "C:/ws", sessionId: "nonce-x" });
  assert(
    win.includes('New-Item -ItemType Directory -Force -Path "C:/ws/.aider/history"'),
    "powershell launch creates the history folder",
  );
  assert(
    win.includes('$env:CODETERM_SESSION_BINDING_NONCE="nonce-x";'),
    "powershell launch exports the binding nonce for the history flags",
  );
  assert(
    win.includes('--chat-history-file "C:/ws/.aider/history/$($env:CODETERM_SESSION_BINDING_NONCE).md"'),
    "powershell launch names the chat history file by the binding nonce",
  );
  assert(
    win.includes('--input-history-file "C:/ws/.aider/history/$($env:CODETERM_SESSION_BINDING_NONCE).input"'),
    "powershell launch names the input history file by the binding nonce",
  );
  assert(
    !win.includes("'$env:CODETERM_SESSION_BINDING_NONCE'") &&
      !win.includes("'$env:CODETERM_SESSION_BINDING_NONCE"),
    "powershell nonce is never single-quoted",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
