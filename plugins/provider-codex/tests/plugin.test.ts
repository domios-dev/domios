/**
 * Codex plugin contract + detection tests.
 * Run: npx tsx plugins/provider-codex/tests/plugin.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";

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

function loadHostPlugin() {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = {
    host: {
      manifestJson: () => "null",
      homeDir: () => "/tmp",
      path: { equal: () => true },
      platform: () => "linux",
      unixNowMs: () => 0,
      envGet: () => null,
      shell: {
        quoteFor: (value: unknown) => String(value),
        quote: (value: unknown) => String(value),
      },
      fs: {
        fileExists: () => false,
        readFile: () => null,
        readFileHead: () => null,
        readFileTail: () => null,
        readJson: () => null,
      },
      expandHome: (value: string) => value,
      writeFile: () => true,
      removeFile: () => true,
      log: () => {},
    },
    console,
    module: { exports: {} },
    exports: {},
  };
  const script = new vm.Script(source, { filename: pluginPath });
  script.runInNewContext(context);
  return context.module.exports.default;
}

function loadCodexWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

function fixtureText(name: string): string {
  return fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", name), "utf8");
}

const manifestSeed = {
  models: [
    { id: "seed-model", displayName: "Seed Model", description: "Manifest fallback" },
  ],
};

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const full = path.join(root, entry.name);
    if (entry.isDirectory()) files.push(...sourceFiles(full));
    else if (/\.(?:ts|tsx)$/.test(entry.name)) files.push(full);
  }
  return files;
}

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp/home",
  platform: () => "linux",
  unixNowMs: () => 1_000_000,
  path: { equal: (a: string, b: string) => a === b },
  expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
  envGet: () => null,
  credentialPublic: () => null,
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readFileHead: () => null,
    readFileTail: () => null,
    readJson: () => null,
    writeJson: () => true,
    writeFile: () => true,
    writeTempFile: () => "/tmp/codeterm task.md",
    walkDir: () => [],
  },
  ...over,
});

console.log("\n--- provider update capability ---\n");

{
  const p = loadHostPlugin();
  eq(
    p.providerSelfUpdateCapability(),
    {
      inSession: true,
      restartRequired: true,
      completionSignal: "provider_classification",
    },
    "in-session updates declare the completion signal required for restart",
  );
}

console.log("\n--- task instructions launch ---\n");

{
  const p = loadCodexWithHost(baseHost({
    platform: () => "windows",
    shell: { quoteFor: (v: unknown) => `'${String(v)}'`, quote: (v: unknown) => `'${String(v)}'` },
    fs: { ...baseHost({}).fs, writeTempFile: () => "C:\\Temp\\codeterm task.md" },
  }));
  const hostileTask = "line one\n<location>D:\\repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод";
  const cmd = p.buildLaunchCommand({ task: hostileTask, args: ["--model", "gpt-5.5"] });
  assert(!cmd.includes("System.Web"), "Windows launch never references System.Web (PowerShell 7 has no .NET Framework assembly to load)");
  assert(!cmd.includes("JavaScriptSerializer"), "Windows launch never references JavaScriptSerializer (Framework-only type)");
  assert(cmd.includes("ConvertTo-Json -Compress"), "Windows serializes instructions with the built-in cmdlet available on both PowerShell editions");
  assert(cmd.includes("$ctTaskInstructions = Get-Content -Raw -Encoding UTF8 'C:\\Temp\\codeterm task.md'"), "Windows loads developer instructions from the task file");
  assert(cmd.includes("$env:CODETERM_TAB_ID -replace '[^A-Za-z0-9_-]', '-'"), "Windows scopes the temporary profile to the tab lifecycle");
  assert(cmd.includes("Set-Content -LiteralPath $ctProfilePath"), "Windows writes a temporary Codex profile");
  const profileValueAssignment = /-Value \('developer_instructions = ' \+ (\$\w+)\)/.exec(cmd);
  assert(!!profileValueAssignment, "the profile content assigns developer_instructions from a variable expression, not a bare literal");
  const jsonVarName = profileValueAssignment?.[1] ?? "";
  assert(
    !!jsonVarName && cmd.includes(`${jsonVarName} = ($ctTaskInstructions | ConvertTo-Json`),
    "the developer_instructions variable is always produced by ConvertTo-Json, so the profile line can never render as a bare 'developer_instructions = '",
  );
  assert(!cmd.includes("Remove-Item -LiteralPath 'C:\\Temp\\codeterm task.md'"), "Windows launch line never deletes the task file, so a relaunch can still read it");
  assert(cmd.includes("--profile $ctProfileName --"), "Windows passes only the short profile name through native argv");
  assert(cmd.includes("finally { Remove-Item -LiteralPath $ctProfilePath"), "Windows removes the temporary profile on exit");
  assert(!cmd.includes('-c ("developer_instructions=" +'), "Windows never expands multiline task text inline in the native call");
  assert(!cmd.includes("<location>") && !cmd.includes("whoami") && !cmd.includes("юникод"), "task body is absent from the launch command");
  const idle = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "idle_wakeup" });
  assert(idle.includes("You are a newly started agent. Wait for instructions from the user or your orchestration parent."), "idle launch uses the non-actionable wake-up starter");
  assert(!idle.includes("Execute the assigned task."), "idle launch never uses the task starter");
  const silent = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "no_starter" });
  assert(silent.includes("--profile $ctProfileName"), "silent launch keeps developer instructions");
  assert(!silent.includes(" -- '"), "silent launch has no positional starter turn");
  const empty = p.buildLaunchCommand({ task: "   " });
  assert(!empty.includes("Execute the assigned task."), "empty instructions never create a starter message");
  const teamText = "host-owned team bootstrap";
  const team = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "team_bootstrap", starterPromptText: teamText });
  assert(team.includes(teamText), "team launch uses the host-owned starter text");
  const handshakeText = "opaque-startup-handshake";
  const handshake = p.buildLaunchCommand({ task: hostileTask, starterPrompt: "startup_handshake", starterPromptText: handshakeText });
  assert(handshake.includes(handshakeText), "startup launch uses the host-owned starter text");
  eq(p.starterPromptText?.("team_bootstrap"), undefined, "team starter text is not duplicated in the plugin");

  // The profile path is computed only from $env:CODETERM_TAB_ID, never a
  // per-call random value, so a relaunch overwrites instead of accumulating.
  const profileComputation = (cmd: string): string => {
    const start = cmd.indexOf("$ctProfileSuffix = if");
    const end = cmd.indexOf("Set-Content -LiteralPath $ctProfilePath");
    assert(start !== -1 && end !== -1 && end > start, "profile-path computation fragment is present");
    return cmd.slice(start, end);
  };
  const firstLaunch = p.buildLaunchCommand({ task: hostileTask, args: ["--model", "gpt-5.5"] });
  const secondLaunch = p.buildLaunchCommand({ task: "a completely different task body", starterPrompt: "team_bootstrap", starterPromptText: "unrelated" });
  eq(
    profileComputation(firstLaunch),
    profileComputation(secondLaunch),
    "the profile path computation depends only on the tab id, so a relaunch targets the same file and overwrites rather than accumulating",
  );

  assert(!firstLaunch.includes("Remove-Item -LiteralPath 'C:\\Temp\\codeterm task.md'"), "the first launch attempt never deletes the task file");
  assert(!secondLaunch.includes("Remove-Item -LiteralPath 'C:\\Temp\\codeterm task.md'"), "a rebuilt relaunch command never deletes the task file either");
  assert(
    secondLaunch.includes("Get-Content -Raw -Encoding UTF8 'C:\\Temp\\codeterm task.md'"),
    "a relaunch still reads the task file from the exact path the prior attempt left in place",
  );
}

console.log("\n--- non-Windows launch branch is unaffected by the Windows fix ---\n");

{
  const p = loadCodexWithHost(baseHost({
    platform: () => "linux",
    fs: { ...baseHost({}).fs, writeTempFile: () => "/tmp/codeterm-task.md" },
  }));
  const cmd = p.buildLaunchCommand({ task: "some task body", args: ["--model", "gpt-5.5"], starterPrompt: "no_starter" });
  eq(
    cmd,
    'codex --model gpt-5.5 -c "developer_instructions=$(cat /tmp/codeterm-task.md)"',
    "the macOS/Linux launch line keeps its exact prior shape",
  );
  assert(!cmd.includes("System.Web") && !cmd.includes("JavaScriptSerializer"), "non-Windows launch never carries the Windows-only serializer reference");
  assert(!cmd.includes("ConvertTo-Json") && !cmd.includes("Set-Content") && !cmd.includes("$ctProfile"), "non-Windows launch has no PowerShell profile machinery");
  assert(!cmd.includes("Remove-Item"), "non-Windows launch never deleted the task file, before or after this fix");
}

console.log("\n--- meta ---\n");
eq(plugin.meta.id, "codex", "meta.id is 'codex'");
eq(plugin.meta.displayName, "Codex", "meta.displayName");

{
  const expectedSurface = [
    "starterPromptText", "detectFromTitle", "detectFromOutput", "screenHasTui", "isOutputNoise",
    "hasEventMarkers", "detectEvents", "parsePrompt", "buildPromptResponse", "directoryTrustPromptResponse",
    "discoverModels", "buildLaunchCommand", "buildResumeCommand", "sessionFilePath", "sessionJsonlPath",
    "parseSessionDelta", "detectSessionId", "checkIntegration", "installIntegration", "grantIntegrationTrust",
    "uninstallIntegration", "restoreIntegration", "ensureIntegrationOnStartup", "parseUsage", "fetchUsage",
    "detectActiveSession", "enumerateSessions", "detectSessionModel", "parseSessionMessages", "classifyTabState",
    "providerSelfUpdateCapability",
  ].sort();
  eq(Object.keys(loadHostPlugin()).sort(), expectedSurface, "plugin split preserves the exported API surface");
  assert(
    sourceFiles(path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src"))
      .every((file) => fs.readFileSync(file, "utf8").split(/\r?\n/).length < 1200),
    "every provider source file stays below the hard line limit",
  );
}

console.log("\n--- detect ---\n");

const codexFixture = [
  "OpenAI Codex",
  "codex/gpt-5",
  "▶ Provide your answer",
  "ctrl+c to cancel",
].join("\n");

assert(plugin.detect({ bytes: "", title: "codex" }), "title 'codex' positive");
assert(plugin.detect({ bytes: "", title: "Codex CLI" }), "title 'Codex CLI' (case-insensitive)");
assert(plugin.detect({ bytes: codexFixture }), "current real multi-signal output detects");
for (const literal of ["OpenAI Codex", "codex/", "ctrl+c to cancel"]) {
  assert(
    plugin.detect({ bytes: codexFixture.replace(literal, "[changed copy]") }),
    `single literal mutation still detects: ${literal}`
  );
}
assert(!plugin.detect({ bytes: "$ ls\nfile.txt", title: "bash" }), "plain shell negative");

{
  const p = loadHostPlugin();
  assert(
    p.screenHasTui("› Explain this codebase\n  gpt-5.5 medium · D:\\Developer\\codeterm"),
    "current prompt and Windows status footer prove a Codex TUI",
  );
  assert(
    p.screenHasTui("› Explain this codebase\n  gpt-5.5 medium · /home/user/codeterm"),
    "current prompt and Unix status footer prove a Codex TUI",
  );
  assert(!p.screenHasTui("PS D:\\Developer\\codeterm>"), "plain shell is not a Codex TUI");
}
assert(!plugin.detect({ bytes: "claude code", title: "claude" }), "claude title is not codex");

console.log("\n--- statusOf ---\n");
eq(plugin.statusOf({ kind: "anything" }), null, "statusOf always returns null (PTY-driven status)");

console.log("\n--- parseUsage (/status screen) ---\n");
{
  const evp = loadHostPlugin();
  const nowMs = new Date(2026, 5, 14, 12, 0, 0, 0).getTime();
  const statusScreen = fixtureText("codex-status-legacy.txt");
  const snap = evp.parseUsage(statusScreen, nowMs) as Record<string, unknown>;
  eq(snap.session_pct, 42, "legacy fixture maps % left to % used for 5h limit");
  eq(snap.weekly_pct, 28, "legacy fixture maps % left to % used for weekly limit");
  assert(snap.session_resets_at_ms != null, "session reset timestamp parsed");
  assert(snap.weekly_resets_at_ms != null, "weekly reset timestamp parsed");
  eq(evp.parseUsage("OpenAI Codex\nloading…", nowMs), null, "parseUsage null without gauges");
}

console.log("\n--- parseUsage: /status gauges ---\n");
{
  const p = loadHostPlugin();
  const now = new Date(2026, 5, 13, 10, 0, 0, 0).getTime();
  const screen = fixtureText("codex-status-v0-142-4.txt");
  const usage = p.parseUsage(screen, now);
  eq(usage?.session_pct, 82, "v0.142.4 fixture keeps GENERAL family as session (not model)");
  eq(usage?.weekly_pct, 56, "v0.142.4 fixture keeps weekly family on explicit label");
  eq(
    usage?.session_resets_at_ms,
    new Date(2026, 5, 13, 23, 17, 0, 0).getTime(),
    "session reset keeps local host-time mapping",
  );
  eq(
    usage?.weekly_resets_at_ms,
    new Date(2026, 6, 14, 7, 45, 0, 0).getTime(),
    "weekly reset keeps local day/month mapping in host timezone",
  );
}

console.log("\n--- quota-free usage fetch ---\n");
{
  let request: Record<string, unknown> | null = null;
  const p = loadCodexWithHost(baseHost({
    fetch: (raw: string) => {
      request = JSON.parse(raw);
      return JSON.stringify({
        status: 200,
        body: JSON.stringify({
          rate_limit: {
            primary_window: { used_percent: 17, limit_window_seconds: 18_000, reset_at: 1_780_000_000 },
            secondary_window: { used_percent: 92, limit_window_seconds: 604_800, reset_at: 1_781_000_000 },
          },
        }),
      });
    },
    credentialPublic: (id: string) =>
      id === "codexAuth" ? JSON.stringify({ accountId: "account-1" }) : null,
  }));
  const raw = p.fetchUsage();
  const usage = p.parseUsage(raw, 1_000_000);
  eq(request?.url, "https://chatgpt.com/backend-api/wham/usage", "fetchUsage uses the provider account meter endpoint");
  eq(usage?.session_pct, 17, "fetchUsage maps the short window");
  eq(usage?.weekly_pct, 92, "fetchUsage maps the weekly window");
  eq(usage?.session_resets_at_ms, 1_780_000_000_000, "fetchUsage maps reset seconds to milliseconds");
  assert(!raw.includes("secret-token"), "fetchUsage output never persists credentials");
}

console.log("\n--- parseUsage: robustness matrix (structural, not literal-copy) ---\n");
{
  const p = loadHostPlugin();
  const now = new Date(2026, 5, 14, 12, 0, 0, 0).getTime();

  // Deliberately shifted format: extra whitespace, reordered spacing,
  // % used + % remaining in one screen, Month-Day reset order.
  const shiftedText = fixtureText("codex-status-shifted.txt");
  const shifted = p.parseUsage(shiftedText, now);
  eq(shifted?.session_pct, 99, "shifted: '% used' passes through unchanged");
  eq(shifted?.weekly_pct, 75, "shifted: weekly '25% remaining' inverts to 75 used");
  assert(shifted?.session_resets_at_ms != null, "shifted: session reset parsed despite padding");
  eq(
    shifted?.weekly_resets_at_ms,
    new Date(2026, 7, 3, 6, 0, 0, 0).getTime(),
    "shifted: 'Aug 3' Month-Day reset order parsed",
  );

  // Alternate 5-hour label word form + '% remaining' + DD-Mon reset order.
  const word = p.parseUsage(fixtureText("codex-status-wordlabel.txt"), now);
  eq(word?.session_pct, 65, "wordlabel: '5 hour limit' + '35% remaining' -> 65 used");
  eq(word?.weekly_pct, 20, "wordlabel: weekly '80% left' -> 20 used");
  assert(word?.session_resets_at_ms != null, "wordlabel: session reset parsed");
  assert(word?.weekly_resets_at_ms != null, "wordlabel: '14 Jun' DD-Mon reset parsed");

  // Same shifted screen with carriage returns must not break parsing.
  const shiftedCrlf = p.parseUsage(shiftedText.replace(/\n/g, "\r\n"), now);
  eq(shiftedCrlf?.session_pct, shifted?.session_pct, "crlf: shifted session survives CRLF");
  eq(shiftedCrlf?.weekly_pct, shifted?.weekly_pct, "crlf: shifted weekly survives CRLF");

  // CRLF-only inline variant with a dash label.
  const crlf = p.parseUsage(
    "OpenAI Codex\r\n5-hour limit ██░ 40% left (resets 19:59)\r\n" +
      "Weekly limit █░ 30% used (resets 19:59 on 14 Jun)\r\n",
    now,
  );
  eq(crlf?.session_pct, 60, "crlf: '5-hour' dash label + '40% left' -> 60 used");
  eq(crlf?.weekly_pct, 30, "crlf: weekly '30% used' passes through");

  // Percent-used semantics guard: 'left'/'remaining' invert, 'used' does not.
  eq(
    p.parseUsage("General 5h limit █ 10% left (resets 19:59)", now)?.session_pct,
    90,
    "semantics: '10% left' -> 90 used",
  );
  eq(
    p.parseUsage("General 5h limit █ 10% used (resets 19:59)", now)?.session_pct,
    10,
    "semantics: '10% used' -> 10 used",
  );
}

console.log("\n--- actions ---\n");
eq(plugin.actions, [], "no actions defined yet");

console.log("\n--- titlePattern + noisePatterns ---\n");
assert(plugin.titlePattern instanceof RegExp, "titlePattern is RegExp");
assert(plugin.titlePattern!.test("codex"), "titlePattern matches 'codex'");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns non-empty");

assert(plugin.imageSend === undefined, "imageSend undefined (host falls back to path strategy)");

console.log("\n--- hasEventMarkers: picker/menu screens gate a screen read (/model) ---\n");
{
  const evp = loadHostPlugin();
  const modelScreen = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "model-picker.txt"),
    "utf8",
  );
  assert(evp.hasEventMarkers(modelScreen), "hasEventMarkers fires on the /model picker screen");
  assert(evp.hasEventMarkers("Select Model and Effort"), "fires on the /model title before rows settle");
  assert(evp.hasEventMarkers("Press enter to confirm or esc to go back"), "fires on the confirm/back footer");
  assert(evp.hasEventMarkers("› 1. gpt-5.5 (current)"), "fires on a › selector option row");
  assert(evp.hasEventMarkers("Would you like to run the following command"), "still fires on approval phrasing");
  assert(!evp.hasEventMarkers("Wrote 12 lines to src/lib/foo.ts and ran the tests."), "stays quiet on normal output");
}

console.log("\n--- parsePrompt + buildPromptResponse: /model numbered picker ---\n");
{
  const evp = loadHostPlugin();
  const modelScreen = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "model-picker.txt"),
    "utf8",
  );
  const parsed = evp.parsePrompt(modelScreen);
  eq(
    parsed && parsed.question,
    "Select Reasoning Level for gpt-5.5",
    "parsePrompt uses the active picker cluster",
  );
  eq(
    parsed && parsed.options,
    [
      ["Low                        Fast responses with lighter reasoning", "1"],
      ["Medium (default) (current) Balances speed and reasoning depth for everyday tasks", "2"],
      ["High                       Greater reasoning depth for complex problems", "3"],
      ["Extra high                 Extra high reasoning depth for complex problems", "4"],
    ],
    "parsePrompt extracts active numbered options with 1-based values",
  );
  eq(evp.buildPromptResponse(1, modelScreen), ["DQ=="], "current highlighted option → Enter only");
  eq(evp.buildPromptResponse(0, modelScreen), ["G1tB", "DQ=="], "target above current → Up + Enter");
  eq(evp.buildPromptResponse(3, modelScreen), ["G1tC", "G1tC", "DQ=="], "target below current → Down ×2 + Enter");
}

console.log("\n--- parsePrompt + buildPromptResponse: prose choice prompts ---\n");
{
  const evp = loadHostPlugin();
  const abc = [
    "• Which release strategy should we use?",
    "  A) Fast patch release now",
    "  B) Minor release with bundled fixes",
    "  C) Delay for more testing",
  ].join("\n");
  const parsed = evp.parsePrompt(abc);
  eq(parsed && parsed.question, "Which release strategy should we use?", "lettered prose question extracts");
  eq(
    parsed && parsed.options,
    [
      ["Fast patch release now", "A"],
      ["Minor release with bundled fixes", "B"],
      ["Delay for more testing", "C"],
    ],
    "lettered prose options extract",
  );
  eq(evp.buildPromptResponse(1, abc), ["Qg0="], "lettered click sends selected value plus Enter");

  const numbered = [
    "• Which test strategy should we use?",
    "",
    "  1. Unit tests first",
    "  2. Integration smoke tests",
    "  3. Full end-to-end suite",
  ].join("\n");
  const parsedNumbered = evp.parsePrompt(numbered);
  eq(parsedNumbered && parsedNumbered.question, "Which test strategy should we use?", "numbered prose question extracts");
  eq(evp.buildPromptResponse(2, numbered), ["Mw0="], "numbered click sends selected value plus Enter");
}

console.log("\n--- parseSessionDelta hardening ---\n");
const hostPlugin = loadHostPlugin();
const chunk = [
  JSON.stringify({
    type: "response_item",
    timestamp: "2026-01-01T00:00:00Z",
    payload: { type: "message", role: "assistant", content: [{ text: "ok", type: "text" }] },
  }),
  JSON.stringify({ type: "response_item", timestamp: "2026-01-01T00:00:00Z", payload: {} }),
].join("\n");
let warned = 0;
let parsed: { messages: unknown[] } = { messages: [] };
const origWarn = console.warn;
try {
  console.warn = () => warned++;
  parsed = hostPlugin.parseSessionDelta(chunk);
} catch (e) {
  assert(false, "parseSessionDelta does not throw on malformed entry", String(e));
} finally {
  console.warn = origWarn;
}
assert(parsed.messages.length === 1, "parseSessionDelta corrupted lines are skipped");
assert(warned === 1, "parseSessionDelta emits one warning for a session with invalid entries");

{
  const sparseChunk = [
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:00Z",
      payload: {
        type: "function_call",
        name: "exec_command",
        arguments: JSON.stringify({ cmd: "npm test" }),
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:01Z",
      payload: {
        type: "function_call",
        name: "send_message",
        arguments: JSON.stringify({ target: "worker", message: "internal handoff" }),
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:02Z",
      payload: {
        type: "function_call",
        name: "exec_command",
        arguments: JSON.stringify({ cmd: "npm run build" }),
      },
    }),
  ].join("\n");
  const sparseParsed = hostPlugin.parseSessionDelta(sparseChunk) as { messages: any[] };
  eq(sparseParsed.messages.length, 2, "parseSessionDelta filters internal orchestration function_call rows");
  eq(
    sparseParsed.messages.map((message) => message.blocks[0].data.input.command),
    ["npm test", "npm run build"],
    "parseSessionDelta keeps real exec_command rows",
  );
}

{
  const agentsText = [
    "# AGENTS.md instructions for /Users/user/Developer/codeterm-alpha",
    "",
    "<INSTRUCTIONS>",
    "# CodeTerm",
    "</INSTRUCTIONS>",
  ].join("\n");
  const abortedText = [
    "<turn_aborted>",
    "The user interrupted the previous turn on purpose.",
    "</turn_aborted>",
  ].join("\n");
  const environmentText = [
    "<environment_context>",
    "  <current_date>2026-07-21</current_date>",
    "  <timezone>Europe/Kiev</timezone>",
    "</environment_context>",
  ].join("\n");
  const systemChunk = [
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:03Z",
      payload: { type: "message", role: "user", content: [{ type: "input_text", text: agentsText }] },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:04Z",
      payload: { type: "message", role: "user", content: [{ type: "input_text", text: abortedText }] },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-01-01T00:00:05Z",
      payload: { type: "message", role: "user", content: [{ type: "input_text", text: environmentText }] },
    }),
  ].join("\n");
  const systemParsed = hostPlugin.parseSessionDelta(systemChunk) as { messages: any[] };
  eq(
    systemParsed.messages.map((message) => [message.role, message.sourceSubtype]),
    [["system", "agent_instructions"], ["system", "turn_aborted"], ["system", "environment_context"]],
    "parseSessionDelta maps Codex injected user-shaped rows to system subtypes",
  );
  eq(
    systemParsed.messages.map((message) => message.blocks[0].data.subtype),
    ["agent_instructions", "turn_aborted", "environment_context"],
    "parseSessionDelta preserves Codex system subtype in text block data",
  );

  const searchParsed = hostPlugin.parseSessionMessages(systemChunk) as any[];
  eq(
    searchParsed.map((message) => [message.role, message.isMeta, message.sourceSubtype]),
    [["system", true, "agent_instructions"], ["system", true, "turn_aborted"], ["system", true, "environment_context"]],
    "parseSessionMessages marks Codex injected rows as system meta",
  );
}

{
  const patchBody = [
    "*** Begin Patch",
    "*** Update File: cli/src/cmd/mem/mod.rs",
    "@@",
    "-old",
    "+new",
    "*** End Patch",
  ].join("\n");
  const structuredChunk = [
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-06-23T18:05:59.394Z",
      payload: {
        type: "custom_tool_call",
        name: "apply_patch",
        call_id: "call_patch_1",
        input: patchBody,
      },
    }),
    JSON.stringify({
      type: "event_msg",
      timestamp: "2026-06-23T18:05:59.398Z",
      payload: {
        type: "patch_apply_end",
        call_id: "call_patch_1",
        stdout: "Success. Updated the following files:\nM cli/src/cmd/mem/mod.rs\n",
        success: true,
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-06-23T18:06:00.000Z",
      payload: {
        type: "function_call",
        name: "exec_command",
        call_id: "call_exec_1",
        arguments: JSON.stringify({ cmd: 'codeterm send "$VAR" --tab target' }),
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-06-23T18:06:00.100Z",
      payload: {
        type: "function_call_output",
        call_id: "call_exec_1",
        output: "src/example.ts:1:needle",
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-06-23T18:06:00.200Z",
      payload: {
        type: "custom_tool_call",
        name: "exec",
        call_id: "call_custom_1",
        input: JSON.stringify({ command: "git status --short" }),
      },
    }),
    JSON.stringify({
      type: "response_item",
      timestamp: "2026-06-23T18:06:00.300Z",
      payload: {
        type: "custom_tool_call_output",
        call_id: "call_custom_1",
        output: [
          { type: "input_text", text: "Script completed\n" },
          { type: "input_text", text: "Exit code: 0\n" },
        ],
      },
    }),
  ].join("\n");
  const structuredParsed = hostPlugin.parseSessionDelta(structuredChunk) as { messages: any[] };
  eq(structuredParsed.messages.length, 6, "tool starts and completions survive parseSessionDelta");
  eq(structuredParsed.messages[0].blocks[0].kind, "tool_use", "custom_tool_call maps to tool_use");
  eq(structuredParsed.messages[0].blocks[0].data.name, "apply_patch", "apply_patch name preserved");
  assert(
    String(structuredParsed.messages[0].blocks[0].data.input.patch).includes("*** Begin Patch"),
    "apply_patch input stored as patch text",
  );
  eq(structuredParsed.messages[0].blocks[0].data.id, "call_patch_1", "call_id forwarded as tool id");
  eq(structuredParsed.messages[1].blocks[0].kind, "tool_result", "patch_apply_end maps to tool_result");
  eq(structuredParsed.messages[1].blocks[0].data.tool_use_id, "call_patch_1", "patch result links to call_id");
  eq(structuredParsed.messages[2].blocks[0].data.input.command, 'codeterm send "$VAR" --tab target', "function call keeps the raw command source");
  eq(structuredParsed.messages[2].blocks[0].data.tool_detail, 'codeterm send "$VAR" --tab target', "canonical tool detail keeps the raw command source");
  eq(structuredParsed.messages[2].blocks[0].data.id, "call_exec_1", "function call id is preserved");
  eq(structuredParsed.messages[3].blocks[0].data.tool_use_id, "call_exec_1", "function result links to call id");
  eq(structuredParsed.messages[3].blocks[0].data.content, "src/example.ts:1:needle", "function output is preserved for event result propagation");
  eq(structuredParsed.messages[4].blocks[0].data.id, "call_custom_1", "custom call id is preserved");
  eq(structuredParsed.messages[5].blocks[0].data.tool_use_id, "call_custom_1", "custom result links to call id");
  eq(structuredParsed.messages[5].blocks[0].data.content, "Script completed\nExit code: 0\n", "custom result text blocks are joined");
}

{
  const patch = "*** Begin Patch\n*** Update File: src/demo.ts\n@@\n-old\n+new\n*** End Patch";
  const directRaw = `text(await tools.apply_patch(${JSON.stringify(patch)}));`;
  const variableRaw = `const patch = ${JSON.stringify(patch)};\ntext(await tools.apply_patch(patch));`;
  const chunk = [directRaw, variableRaw].map((raw, index) => JSON.stringify({
    type: "response_item",
    payload: {
      type: "custom_tool_call",
      name: "exec",
      call_id: `call_nested_patch_${index}`,
      input: JSON.stringify({ raw }),
    },
  })).join("\n");
  const parsed = hostPlugin.parseSessionDelta(chunk) as { messages: any[] };

  eq(parsed.messages.length, 2, "nested patch calls survive parseSessionDelta");
  for (const message of parsed.messages) {
    eq(message.blocks[0].data.name, "apply_patch", "nested exec normalizes to apply_patch");
    eq(message.blocks[0].data.input.patch, patch, "nested exec preserves patch text for DiffView");
    eq(message.blocks[0].data.tool_kind, "edit", "nested apply_patch emits edit kind");
    eq(message.blocks[0].data.tool_edits[0].path, "src/demo.ts", "nested apply_patch emits edit path");
  }
}

{
  const command = String.raw`const r = await tools.shell_command({command:"powershell -Command \"Write-Host 'ready'; Get-ChildItem -Filter \\\"*.ts\\\"\"",workdir:"D:\\x",timeout_ms:20000}); text(r)`;
  const malformed = String.raw`const r = await tools.shell_command({command:"unterminated}); text(r)`;
  const parsed = hostPlugin.parseSessionDelta([
    JSON.stringify({ type: "response_item", payload: { type: "custom_tool_call", name: "exec", input: JSON.stringify({ raw: command }) } }),
    JSON.stringify({ type: "response_item", payload: { type: "custom_tool_call", name: "exec", input: JSON.stringify({ raw: malformed }) } }),
  ].join("\n")) as { messages: any[] };
  const commandData = parsed.messages[0].blocks[0].data;
  eq(commandData.tool_kind, "command", "nested shell wrapper emits command kind");
  eq(commandData.tool_summary, `powershell -Command "Write-Host 'ready'; Get-ChildItem -Filter \\\"*.ts\\\""`, "nested shell wrapper preserves escaped command");
  eq(commandData.tool_detail, commandData.tool_summary, "command detail is the full command string");
  const malformedData = parsed.messages[1].blocks[0].data;
  eq(malformedData.tool_kind, "generic", "malformed shell wrapper falls back to generic");
  assert(!String(malformedData.tool_summary).includes("{"), "malformed shell wrapper does not expose JSON summary");
}

{
  const calls = [
    `const r = await tools.exec_command({cmd:"./build.sh check",workdir:"/repo"}); text(r.output);`,
    `const results = await Promise.all([tools.exec_command({cmd:"rg -n provider src"}),tools.exec_command({cmd:"sed -n '1,80p' src/main.ts"})]); for (const r of results) text(r.output);`,
    `const r = await tools.write_stdin({session_id:42,chars:"",yield_time_ms:1000}); text(r.output);`,
    `const r = await tools.view_image({path:"/tmp/update-error.png",detail:"original"}); image(r.image_url);`,
    "const r = await tools.view_image({path:\"C:\\\\work\\\\update-error.png\",detail:\"original\"});\r\nimage(r.image_url);",
    "first readable line\r\nsecond line",
  ];
  const parsed = hostPlugin.parseSessionDelta(calls.map((input, index) => JSON.stringify({
    type: "response_item",
    payload: { type: "custom_tool_call", name: "exec", call_id: `call_readable_${index}`, input },
  })).join("\n")) as { messages: any[] };
  const data = parsed.messages.map((message) => message.blocks[0].data);

  eq(data[0].tool_kind, "command", "nested exec_command renders as a command");
  eq(data[0].tool_summary, "./build.sh check", "nested exec_command exposes its command");
  eq(data[1].tool_summary, "rg -n provider src (+1 more)", "parallel commands keep a readable summary");
  eq(data[2].tool_summary, "Wait for command output", "empty write_stdin is described as a wait");
  eq(data[3].tool_kind, "read", "view_image renders as a read action");
  eq(data[3].tool_summary, "/tmp/update-error.png", "view_image exposes the inspected path");
  eq(data[4].tool_summary, "C:\\work\\update-error.png", "Windows paths survive readable summaries");
  eq(data[5].tool_summary, "first readable line", "unknown exec scripts fall back to one line");
}

console.log("\n--- binding nonce (install + outdated detection) ---\n");

// session-start report can prove tab ownership (release/1.4.6 binding-nonce).
{
  let writtenJson: any = null;
  let writtenScript = "";
  const host = baseHost({
    platform: () => "windows",
    fs: {
      fileExists: () => false,
      readJson: () => null,
      readFile: () => null,
      writeJson: (_p: string, json: unknown) => { writtenJson = json; return true; },
      writeFile: (_p: string, body: string) => { writtenScript = body; return true; },
    },
  });
  const p = loadCodexWithHost(host);
  const res = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(!!res && res.ok === true, "installIntegration succeeds", JSON.stringify(res));
  const cmd = String(writtenJson?.hooks?.SessionStart?.[0]?.hooks?.[0]?.command ?? "");
  assert(
    cmd.indexOf("X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}") !== -1,
    "session_start hook curl carries the binding-nonce header",
    cmd,
  );
  assert(
    writtenScript.includes("codeterm-profile-cleanup-v1") &&
      writtenScript.includes('codeterm-$profileSuffix.config.toml') &&
      writtenScript.includes("Remove-Item -LiteralPath $profilePath"),
    "Windows session_start removes the tab-scoped profile after Codex loads it",
    writtenScript,
  );
}

{
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const original = '{"foreign":{"keep":true},"hooks":{"SessionStart":[{"hooks":[{"command":"user-hook"}]}]}}';
  const files = new Map<string, string>([[hooksPath, original]]);
  const p = loadCodexWithHost(baseHost({
    platform: () => "linux",
    fs: {
      fileExists: (path: string) => files.has(path),
      readFile: (path: string) => files.get(path) ?? null,
      readJson: (path: string) => {
        const value = files.get(path);
        return value ? JSON.parse(value) : null;
      },
      writeFile: (path: string, value: string) => { files.set(path, value); return true; },
      writeJson: (path: string, value: unknown) => { files.set(path, JSON.stringify(value)); return true; },
      removeFile: (path: string) => files.delete(path),
    },
  }));
  const denied = p.installIntegration(7686, { consent: false });
  assert(denied.ok === false && files.size === 1, "integration writes nothing before consent");
  const installed = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(installed.ok === true, "consented integration install succeeds");
  eq(files.get(hooksPath + ".codeterm-backup"), original, "backup preserves original bytes");
  const merged = JSON.parse(files.get(hooksPath) || "{}");
  assert(merged.foreign.keep === true, "install preserves foreign configuration");
  assert(merged.hooks.SessionStart.some((entry: any) => entry.hooks?.[0]?.command === "user-hook"), "install preserves foreign hooks");
  assert(p.uninstallIntegration().ok === true, "uninstall succeeds");
  const uninstalled = JSON.parse(files.get(hooksPath) || "{}");
  assert(uninstalled.hooks.SessionStart.some((entry: any) => entry.hooks?.[0]?.command === "user-hook"), "uninstall preserves foreign hooks");
  assert(p.restoreIntegration().ok === true, "restore succeeds");
  eq(files.get(hooksPath), original, "restore reproduces original bytes");
}

for (const [name, fixture] of [
  ["malformed", "{not-json"],
  ["non-object", "[]"],
  ["empty", ""],
] as const) {
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const files = new Map<string, string>([[hooksPath, fixture]]);
  let writes = 0;
  const p = loadCodexWithHost(baseHost({
    platform: () => "linux",
    fs: {
      fileExists: (path: string) => files.has(path),
      readFile: (path: string) => files.get(path) ?? null,
      readJson: () => null,
      writeFile: () => { writes += 1; return true; },
      writeJson: () => { writes += 1; return true; },
      removeFile: () => false,
    },
  }));
  const result = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(result.ok === false, `${name} hooks JSON is rejected`);
  eq(writes, 0, `${name} rejection has no side effects`);
  eq(files.get(hooksPath), fixture, `${name} input bytes remain unchanged`);
}

{
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const configPath = "/tmp/home/.codex/config.toml";
  const ownershipPath = configPath + ".codeterm-hook-trust-owned";
  const trustHeader = `[hooks.state."${hooksPath}:session_start:owned"]`;
  const files = new Map<string, string>([
    [hooksPath, "{}"],
    [configPath, `${trustHeader}\nenabled = false\nforeign = "before"\n`],
  ]);
  const host = baseHost({
    platform: () => "linux",
    fs: {
      fileExists: (path: string) => files.has(path),
      readFile: (path: string) => files.get(path) ?? null,
      readJson: (path: string) => {
        const value = files.get(path);
        return value === undefined ? null : JSON.parse(value);
      },
      writeFile: (path: string, value: string) => { files.set(path, value); return true; },
      writeJson: (path: string, value: unknown) => { files.set(path, JSON.stringify(value)); return true; },
      removeFile: (path: string) => files.delete(path),
    },
  });
  const p = loadCodexWithHost(host);
  assert(p.installIntegration(7686, { consent: true }).ok === true, "owned trust install succeeds");
  assert(files.get(configPath)?.includes("enabled = true") === true, "install expands matching trust");
  eq(files.get(ownershipPath), hooksPath, "owned trust expansion is recorded");
  files.set(configPath, `${files.get(configPath)}foreign_after = "preserve"\n`);
  assert(p.uninstallIntegration().ok === true, "owned trust uninstall succeeds");
  const after = files.get(configPath) || "";
  assert(after.includes("enabled = false"), "uninstall revokes only CodeTerm-owned trust");
  assert(after.includes('foreign_after = "preserve"'), "uninstall preserves later foreign config edits");
  assert(!files.has(ownershipPath), "owned trust marker is removed after revocation");
}

{
  const hooksPath = "/tmp/home\\.codex\\hooks.json";
  const configPath = "/tmp/home\\.codex\\config.toml";
  const scriptPath = "/tmp/home\\.codex\\codeterm-session-start.ps1";
  const ownershipPath = configPath + ".codeterm-hook-trust-owned";
  const initial = new Map<string, string>([
    [hooksPath, '{"foreign":"hooks"}'],
    [configPath, `[hooks.state."${hooksPath}:session_start:owned"]\nenabled = false\nforeign = "config"\n`],
    [scriptPath, "# foreign script\r\n"],
    [ownershipPath, "foreign ownership bytes"],
    [hooksPath + ".codeterm-backup", "prior hooks backup"],
    [configPath + ".codeterm-backup", "prior config backup"],
    [scriptPath + ".codeterm-backup", "prior script backup"],
  ]);
  for (let failAt = 1; failAt <= 9; failAt++) {
    const files = new Map(initial);
    let writeAttempt = 0;
    const write = (path: string, value: string) => {
      writeAttempt += 1;
      if (writeAttempt === failAt) return false;
      files.set(path, value);
      return true;
    };
    const p = loadCodexWithHost(baseHost({
      platform: () => "windows",
      fs: {
        fileExists: (path: string) => files.has(path),
        readFile: (path: string) => files.get(path) ?? null,
        readJson: (path: string) => JSON.parse(files.get(path) || "{}"),
        writeFile: write,
        writeJson: (path: string, value: unknown) => write(path, JSON.stringify(value)),
        removeFile: (path: string) => files.delete(path),
      },
    }));
    const result = p.installIntegration(7686, { consent: true });
    assert(result.ok === false, `Codex install fails at write ${failAt}`);
    eq([...files.entries()].sort(), [...initial.entries()].sort(), `write ${failAt} restores every exact prior byte`);
  }
}

{
  const hooksPath = "/tmp/home\\.codex\\hooks.json";
  const scriptPath = "/tmp/home\\.codex\\codeterm-session-start.ps1";
  const files = new Map<string, string>([
    [hooksPath, '{"hooks":{"SessionStart":[]}}'],
    [scriptPath, "# user-owned replacement\nWrite-Host foreign\n"],
  ]);
  const p = loadCodexWithHost(baseHost({
    platform: () => "windows",
    fs: {
      fileExists: (path: string) => files.has(path),
      readFile: (path: string) => files.get(path) ?? null,
      readJson: (path: string) => JSON.parse(files.get(path) || "{}"),
      writeFile: (path: string, value: string) => { files.set(path, value); return true; },
      writeJson: (path: string, value: unknown) => { files.set(path, JSON.stringify(value)); return true; },
      removeFile: (path: string) => files.delete(path),
    },
  }));
  assert(p.uninstallIntegration().ok === true, "uninstall with foreign replacement succeeds");
  assert(files.has(scriptPath), "uninstall preserves a script that no longer proves CodeTerm ownership");
}

{
  const staleScript = "# codeterm-codex-session-binding-v1\n$env:CODETERM_SESSION_BINDING_NONCE";
  const p = loadCodexWithHost(baseHost({
    platform: () => "windows",
    fs: {
      fileExists: (path: string) => path.endsWith("hooks.json") || path.endsWith("codeterm-session-start.ps1"),
      readJson: () => ({
        hooks: {
          SessionStart: [{ hooks: [{
            _source: "codeterm-codex-session-binding-v1",
            commandWindows: 'powershell -File "codeterm-session-start.ps1"',
          }] }],
        },
      }),
      readFile: (path: string) => path.endsWith("codeterm-session-start.ps1") ? staleScript : "",
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  assert(
    p.checkIntegration().session_start_hook === false,
    "Windows integration rejects a pre-cleanup hook so startup reinstalls it",
  );
}

{
  const stalePaneScript = [
    "# codeterm-codex-session-binding-v1",
    "# codeterm-profile-cleanup-v1",
    "$tab = $env:CODETERM_PANE_ID",
    "$nonce = $env:CODETERM_SESSION_BINDING_NONCE",
    'Invoke-RestMethod -Uri "$base/api/panes/$tab/hook/session_start"',
  ].join("\n");
  const p = loadCodexWithHost(baseHost({
    platform: () => "windows",
    fs: {
      fileExists: (path: string) => path.endsWith("hooks.json") || path.endsWith("codeterm-session-start.ps1"),
      readJson: () => ({
        hooks: {
          SessionStart: [{ hooks: [{
            _source: "codeterm-codex-session-binding-v1",
            commandWindows: 'powershell -File "codeterm-session-start.ps1"',
          }] }],
        },
      }),
      readFile: (path: string) => path.endsWith("codeterm-session-start.ps1") ? stalePaneScript : "",
      writeJson: () => true,
      writeFile: () => true,
    },
  }));
  assert(
    p.checkIntegration().session_start_hook === false,
    "Windows integration rejects a nonce-bearing Pane-era hook",
  );
}

// Startup remains read-only; a hook carrying the binding nonce is current.
{
  const sessionStartHook = (cmd: string) =>
    baseHost({
      fs: {
        fileExists: () => true,
        readFile: () => "",
        readJson: () => ({ hooks: { SessionStart: [{ hooks: [{ command: cmd }] }] } }),
        writeJson: () => true,
        writeFile: () => true,
      },
    });
  const preNonce =
    "curl -s -X POST \"$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start\" " +
    "-H 'X-CodeTerm-Agent-Provider: codex' " +
    '-H "Authorization: Bearer ${CODETERM_API_TOKEN}" --data-binary @-';
  const old = loadCodexWithHost(sessionStartHook(preNonce));
  assert(
    old.checkIntegration().session_start_hook === false,
    "pre-nonce hook is reported not-current (forces reinstall)",
  );
  const current = loadCodexWithHost(
    sessionStartHook(preNonce + ' -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}"'),
  );
  assert(
    current.checkIntegration().session_start_hook === true,
    "hook carrying the binding-nonce header reads as current",
  );
}

{
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const configPath = "/tmp/home/.codex/config.toml";
  const hookJson = {
    hooks: {
      SessionStart: [{
        hooks: [{
          command:
            "curl -s -X POST http://localhost:7686/api/tabs/$CODETERM_TAB_ID/hook/session_start " +
            "-H 'X-CodeTerm-Agent-Provider: codex' " +
            '-H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE}" --data-binary @-',
        }],
      }],
    },
  };
  let config = [
    "[hooks.state]",
    "",
    `[hooks.state."${hooksPath}:session_start:0:0"]`,
    'trusted_hash = "sha256:test"',
    "enabled = false",
    "",
  ].join("\n");
  const p = loadCodexWithHost(
    baseHost({
      fs: {
        fileExists: (p: string) => p === hooksPath || p === configPath,
        readJson: (p: string) => (p === hooksPath ? hookJson : null),
        readFile: (p: string) => (p === configPath ? config : ""),
        writeJson: () => true,
        writeFile: (p: string, body: string) => {
          if (p === configPath) {
            config = body;
          }
          return true;
        },
      },
    }),
  );

  const status = p.checkIntegration();
  assert(status.session_start_hook === false, "disabled Codex hook trust makes integration unhealthy");
  assert(status.hook_trust_enabled === false, "disabled Codex hook trust is surfaced explicitly");
  assert(status.integration_state === "untrusted_hash", "disabled Codex hook trust has its own state constant");

  const checked = p.ensureIntegrationOnStartup(7686);
  assert(checked.ok === true && checked.changed === false, "startup integration check performs no write");
  assert(config.indexOf("enabled = false") !== -1, "startup preserves disabled hook trust without consent");
  assert(p.checkIntegration().session_start_hook === false, "read-only startup leaves explicit user state unchanged");
}

{
  const p = loadCodexWithHost(
    baseHost({
      platform: () => "windows",
      shell: {
        quoteFor: (v: unknown, platform?: unknown) => `Q(${platform}:${String(v)})`,
        quote: (v: unknown) => `BAD(${String(v)})`,
      },
    }),
  );
  eq(
    p.buildResumeCommand("11111111-1111-1111-1111-111111111111"),
    "codex resume Q(windows:11111111-1111-1111-1111-111111111111)",
    "buildResumeCommand quotes session id with quoteFor(platform)",
  );
}

{
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const stale = JSON.stringify({
    hooks: {
      SessionStart: [{ hooks: [{
        _source: "codeterm-codex-session-binding-v1",
        command: "curl -s -X POST http://localhost:7686/api/tabs/$CODETERM_TAB_ID/hook/session_start",
      }] }],
      Stop: [],
    },
  });
  const files = new Map([[hooksPath, stale]]);
  let rewrites = 0;
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (file: string) => files.has(file),
      readFile: (file: string) => files.get(file) ?? null,
      readJson: (file: string) => JSON.parse(files.get(file) || "{}"),
      writeJson: (file: string, value: unknown) => {
        rewrites++;
        files.set(file, JSON.stringify(value));
        return true;
      },
      writeFile: (file: string, value: string) => { files.set(file, value); return true; },
      removeFile: (file: string) => files.delete(file),
    },
  }));
  const result = p.ensureIntegrationOnStartup(7686);
  const rewritten = files.get(hooksPath) || "";
  assert(result.ok === true && result.changed === true, "refresh reports a stale owned hook as rewritten");
  assert(rewrites === 1, "refresh reaches the hook rewrite seam");
  assert(rewritten !== stale, "refresh changes the stale hook bytes");
  assert(rewritten.indexOf("$CODETERM_API_URL") !== -1, "refreshed hook routes through the instance URL variable");
  assert(rewritten.indexOf("${CODETERM_API_TOKEN}") !== -1, "refreshed hook routes through the instance token variable");
  assert(rewritten.indexOf("$CODETERM_TAB_ID") !== -1, "refreshed hook routes through the instance tab variable");
  assert(rewritten.indexOf("${CODETERM_API_URL:-}") !== -1, "refreshed hook fails closed when instance variables are absent");
  assert(!/:\d{2,5}/.test(rewritten), "refreshed hook contains no literal port");
}

{
  const hooksPath = "/tmp/home/.codex/hooks.json";
  const foreign = JSON.stringify({
    hooks: {
      SessionStart: [{ hooks: [{
        _source: "another-integration",
        command: "curl -s -X POST http://localhost:7686/api/tabs/$CODETERM_TAB_ID/hook/session_start",
      }] }],
    },
  });
  const files = new Map([[hooksPath, foreign]]);
  let writes = 0;
  const p = loadCodexWithHost(baseHost({
    fs: {
      fileExists: (file: string) => files.has(file),
      readFile: (file: string) => files.get(file) ?? null,
      readJson: (file: string) => JSON.parse(files.get(file) || "{}"),
      writeJson: () => { writes++; return true; },
      writeFile: () => { writes++; return true; },
      removeFile: () => true,
    },
  }));
  const result = p.ensureIntegrationOnStartup(7686);
  assert(result.ok === true && result.changed === false, "refresh ignores a hook with a foreign marker");
  assert(writes === 0, "foreign hook bytes are never rewritten");
  assert(files.get(hooksPath) === foreign, "foreign hook content remains byte-identical");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
