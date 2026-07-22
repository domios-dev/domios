/**
 * Plugin contract tests for Claude — meta, statusOf, actions, detect.
 * Run: npx tsx plugins/provider-claude/tests/plugin.test.ts
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
        readJson: () => null,
        writeTempFile: () => "/tmp/ct-task",
        writeJson: () => true,
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

function loadClaudeWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const manifestSeed = {
  models: [
    { id: "opus", displayName: "Claude Opus 4.8 (1M context)", description: "Most capable for complex work." },
    { id: "sonnet", displayName: "Claude Sonnet 4.6", description: "Best for everyday tasks. Default for most work." },
    { id: "haiku", displayName: "Claude Haiku 4.5", description: "Fastest for quick answers." },
    { id: "fable", displayName: "Claude Fable 5", description: "Newest model — enabled once available for your account.", available: false },
  ],
};

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp",
  path: { equal: () => true },
  platform: () => "linux",
  unixNowMs: () => 0,
  envGet: () => null,
  fetch: () => JSON.stringify({ error: "not configured" }),
  shell: {
    quoteFor: (value: unknown) => String(value),
    quote: (value: unknown) => String(value),
  },
  fs: {
    fileExists: () => false,
    readFile: () => null,
    readJson: () => null,
    writeTempFile: () => "/tmp/ct-task",
    writeJson: () => true,
  },
  expandHome: (value: string) => value,
  writeFile: () => true,
  removeFile: () => true,
  log: () => {},
  ...over,
});

console.log("\n--- meta ---\n");

eq(plugin.meta.id, "claude", "meta.id is 'claude'");
eq(plugin.meta.displayName, "Claude Code", "meta.displayName");

console.log("\n--- statusOf delegation ---\n");

eq(
  plugin.statusOf({ kind: "user_message" }),
  { kind: "agent_working" },
  "plugin.statusOf delegates to status.ts"
);

console.log("\n--- actions ---\n");

assert(plugin.actions.length >= 2, "has at least 2 actions");

const compact = plugin.actions.find(a => a.id === "compact");
assert(compact !== undefined, "has 'compact' action");
eq(compact?.label, "/compact", "compact action label");
eq(compact?.run({ events: [] }), "/compact\n", "compact action returns slash-command with newline");

const exit = plugin.actions.find(a => a.id === "exit");
assert(exit !== undefined, "has 'exit' action");
eq(exit?.run({ events: [] }), "/exit\n", "exit action returns slash-command with newline");

console.log("\n--- detect delegation ---\n");

assert(plugin.detect({ bytes: "", title: "claude" }), "plugin.detect delegates to detect.ts");

const hostPlugin = loadHostPlugin();

eq(
  hostPlugin.describeSession(
    "/tmp/session.jsonl",
    [
      JSON.stringify({ type: "mode", mode: "normal" }),
      JSON.stringify({ type: "file-history-snapshot", snapshot: {} }),
      JSON.stringify({ type: "attachment", cwd: "D:\\Developer\\codeterm" }),
    ].join("\n"),
  ),
  { projectPath: "D:\\Developer\\codeterm" },
  "describeSession finds cwd after metadata-only rows",
);

eq(
  hostPlugin.describeSession("/tmp/session.jsonl", "not-json\n{}"),
  null,
  "describeSession stays honest when cwd is absent",
);

console.log("\n--- wizard parsePrompt + buildPromptResponse ---\n");

{
  const wizard = fs.readFileSync(
    path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "wizard-multi-step.txt"),
    "utf8",
  );
  const parsed = hostPlugin.parsePrompt(wizard);
  eq(parsed?.question, "Which environment should this setup target?", "wizard parsePrompt extracts current question");
  eq(
    parsed?.sections?.map((s: { label: string; active?: boolean; completed?: boolean }) => [s.label, !!s.active, !!s.completed]),
    [
      ["Environment", true, false],
      ["Autonomy", false, false],
      ["Scope", false, false],
      ["Verbosity", false, false],
    ],
    "wizard parsePrompt extracts sections and active step",
  );
  eq(parsed?.options?.map((o: [string, string]) => o[1]), ["1", "2", "3", "4"], "wizard options keep 1-based values");
  eq(parsed?.multiSelect, false, "wizard numbered section is single-select");
  eq(parsed?.submitAvailable, true, "wizard submit is available");
  eq(hostPlugin.buildPromptResponse(2, wizard), ["ag==", "ag==", "DQ==", "CQ=="], "wizard option click moves from highlighted row, selects, then advances");
  eq(hostPlugin.buildPromptResponse(8999, wizard), ["G1tD", "G1tD", "G1tD", "G1tD", "DQ=="], "wizard submit navigates to Submit then enters");
}

{
  const multi = [
    "←  ✔ Environment  ☐ Focus  ✔ Submit →",
    "Which focus areas should be enabled?",
    "❯ ☐ Performance",
    "  ☑ Tests",
    "  ☐ Docs",
    "",
    "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
  ].join("\n");
  const parsed = hostPlugin.parsePrompt(multi);
  eq(parsed?.multiSelect, true, "checkbox section is multi-select");
  eq(parsed?.selectedValues, ["2"], "checkbox parser extracts selected values");
  eq(hostPlugin.buildPromptResponse(10004, multi), ["IA==", "ag==", "IA==", "ag==", "IA==", "CQ=="], "multi-select confirm toggles desired mask then advances");
}

{
  const trust = [
    "Accessing workspace:",
    "C:\\work\\new-project",
    "Quick safety check: Is this a project you created or one you trust?",
    "❯ 1. Yes, I trust this folder",
    "  2. No, exit",
    "Enter to confirm · Esc to cancel",
  ].join("\n");
  const parsed = hostPlugin.parsePrompt(trust);
  eq(parsed?.question, "Quick safety check: Is this a project you created or one you trust?", "directory trust gate parses as a provider prompt");
  eq(hostPlugin.directoryTrustPromptResponse(parsed, trust), ["DQ=="], "auto-approved directory trust selects the highlighted affirmative option");
}

console.log("\n--- imageSend strategy ---\n");

eq(plugin.imageSend?.method, "clipboard-paste", "imageSend method is clipboard-paste");
eq(plugin.imageSend?.pasteKeystroke, "\x16", "imageSend pasteKeystroke is Ctrl+V (0x16)");

console.log("\n--- titlePattern + noisePatterns exposed ---\n");

assert(plugin.titlePattern instanceof RegExp, "titlePattern is a RegExp");
assert(Array.isArray(plugin.noisePatterns) && plugin.noisePatterns.length > 0, "noisePatterns is non-empty array");

console.log("\n--- parseSessionDelta hardening ---\n");
const chunk = [
  JSON.stringify({ type: "assistant", uuid: "ok", message: { content: [{ type: "text", text: "good" }] } }),
  '{"type":"assistant","message":"not-an-array-or-string"}',
  "",
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

console.log("\n--- parseSessionDelta compact metadata ---\n");
{
  const compactText = "This session is being continued from a previous conversation.";
  const compact = hostPlugin.parseSessionDelta([
    JSON.stringify({
      type: "user",
      uuid: "compact-user",
      isCompactSummary: true,
      isVisibleInTranscriptOnly: true,
      message: { role: "user", content: compactText },
    }),
    JSON.stringify({
      type: "system",
      subtype: "compact_boundary",
      level: "info",
      uuid: "compact-boundary",
    }),
  ].join("\n")) as { messages: Array<Record<string, unknown>> };

  eq(
    compact.messages.map((m) => ({
      role: m.role,
      uuid: m.uuid,
      sourceType: m.sourceType,
      sourceSubtype: m.sourceSubtype,
      level: m.level,
      isCompactSummary: m.isCompactSummary,
      isVisibleInTranscriptOnly: m.isVisibleInTranscriptOnly,
      text: ((m.blocks as Array<{ data: { text?: string; subtype?: string } }>)[0]).data.text,
      subtype: ((m.blocks as Array<{ data: { text?: string; subtype?: string } }>)[0]).data.subtype,
    })),
    [
      {
        role: "system",
        uuid: "compact-user",
        sourceType: "user",
        sourceSubtype: "compact_summary",
        level: null,
        isCompactSummary: true,
        isVisibleInTranscriptOnly: true,
        text: compactText,
        subtype: "compact_summary",
      },
      {
        role: "system",
        uuid: "compact-boundary",
        sourceType: "system",
        sourceSubtype: "compact_boundary",
        level: "info",
        isCompactSummary: null,
        isVisibleInTranscriptOnly: null,
        text: "",
        subtype: "compact_boundary",
      },
    ],
    "compact summaries and boundaries preserve Claude row metadata",
  );
}

console.log("\n--- parseSessionDelta task notification ---\n");
{
  const parsed = hostPlugin.parseSessionDelta(JSON.stringify({
    type: "user",
    uuid: "task-notification",
    origin: { kind: "task-notification" },
    message: {
      role: "user",
      content: "<task-notification>\n<status>failed</status>\n<summary>Background command failed</summary>\n</task-notification>",
    },
  })) as { messages: Array<Record<string, unknown>> };
  eq(
    parsed.messages.map((m) => ({
      role: m.role,
      sourceSubtype: m.sourceSubtype,
      text: ((m.blocks as Array<{ data: { text?: string; subtype?: string } }>)[0]).data.text,
      subtype: ((m.blocks as Array<{ data: { text?: string; subtype?: string } }>)[0]).data.subtype,
    })),
    [
      {
        role: "status",
        sourceSubtype: "task_notification",
        text: "Task failed",
        subtype: "task_notification",
      },
    ],
    "task notifications normalize to status rows in the Claude provider",
  );

  const attachmentParsed = hostPlugin.parseSessionDelta(JSON.stringify({
    type: "attachment",
    uuid: "task-notification-attachment",
    attachment: {
      type: "queued_command",
      commandMode: "task-notification",
      prompt: "<task-notification>\n<status>completed</status>\n<summary>Background command completed</summary>\n</task-notification>",
    },
  })) as { messages: Array<Record<string, unknown>> };
  eq(
    attachmentParsed.messages.map((m) => ({
      role: m.role,
      sourceSubtype: m.sourceSubtype,
      text: ((m.blocks as Array<{ data: { text?: string; subtype?: string } }>)[0]).data.text,
      subtype: ((m.blocks as Array<{ data: { text?: string; subtype?: string } }>)[0]).data.subtype,
    })),
    [
      {
        role: "status",
        sourceSubtype: "task_notification",
        text: "Task completed",
        subtype: "task_notification",
      },
    ],
    "queued task-notification attachments normalize to status rows",
  );
}

console.log("\n--- canonical tool fields ---\n");
{
  const parsed = hostPlugin.parseSessionDelta(JSON.stringify({ type: "assistant", message: { content: [
    { type: "tool_use", name: "Bash", input: { command: "git status" } },
    { type: "tool_use", name: "Edit", input: { file_path: "src/demo.ts", old_string: "old", new_string: "new" } },
    { type: "tool_use", name: "MultiEdit", input: { file_path: "src/multi.ts", edits: [{ old_string: "one", new_string: "two" }, { old_string: "three", new_string: "four" }] } },
    { type: "tool_use", name: "Mystery", input: { value: "opaque" } },
  ] } })) as { messages: Array<{ blocks: Array<{ data: Record<string, unknown> }> }> };
  const data = parsed.messages[0].blocks.map((block) => block.data);
  eq(data[0].tool_kind, "command", "Claude Bash gets canonical command kind");
  eq(data[0].tool_summary, "git status", "Claude Bash gets clean command summary");
  eq(data[1].tool_kind, "edit", "Claude Edit gets canonical edit kind");
  assert(typeof data[1].tool_detail === "string", "Claude tool_detail stays a string");
  eq(data[1].tool_edits, [{ path: "src/demo.ts", old: "old", new: "new" }], "Claude Edit emits canonical path and edit pair");
  eq(data[2].tool_edits, [
    { path: "src/multi.ts", old: "one", new: "two" },
    { path: "src/multi.ts", old: "three", new: "four" },
  ], "Claude MultiEdit propagates parent snake_case path");
  eq(data[3].tool_kind, "generic", "unknown Claude tool degrades to generic");
  assert(typeof data[3].tool_summary === "string" && !(data[3].tool_summary as string).includes("{"), "unknown Claude summary is not JSON");
}

console.log("\n--- discoverModels ---\n");

{
  const p = loadClaudeWithHost(baseHost({
    envGet: (name: string) => name === "ANTHROPIC_API_KEY" ? "test-key" : null,
    fetch: () => JSON.stringify({
      status: 200,
      body: JSON.stringify({
        data: [
          { id: "claude-opus-4-9", display_name: "Claude Opus 4.9" },
          { id: "claude-fable-5", display_name: "Claude Fable 5" },
        ],
      }),
    }),
  }));
  eq(
    p.discoverModels(),
    [
      { id: "opus", displayName: "Claude Opus 4.9 (1M context)", description: "Most capable for complex work.", available: true },
      { id: "sonnet", displayName: "Claude Sonnet 4.6", description: "Best for everyday tasks. Default for most work.", available: false },
      { id: "haiku", displayName: "Claude Haiku 4.5", description: "Fastest for quick answers.", available: false },
      { id: "fable", displayName: "Claude Fable 5", description: "Newest model — enabled once available for your account.", available: true },
    ],
    "discoverModels maps live /v1/models onto curated aliases (version label + fable availability)",
  );
}

{
  const p = loadClaudeWithHost(baseHost({}));
  eq(p.discoverModels(), manifestSeed.models, "discoverModels falls back to manifest seed when no Anthropic key exists");
}

console.log("\n--- Windows task launch shape ---\n");
{
  const p = loadClaudeWithHost(baseHost({
    platform: () => "windows",
    fs: {
      fileExists: () => false,
      readFile: () => null,
      readJson: () => null,
      writeTempFile: () => "C:\\Temp\\ct-task",
      writeJson: () => true,
    },
    shell: {
      quoteFor: (value: unknown) => `'${String(value).replace(/'/g, "''")}'`,
      quote: (value: unknown) => `'${String(value).replace(/'/g, "''")}'`,
    },
  }));
  const hostileTask = "line one\n<location>D:\\repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод";
  const cmd = p.buildLaunchCommand({
    skipPermissions: true,
    args: ["--model", "sonnet"],
    task: hostileTask,
  });
  assert(cmd.includes("--append-system-prompt-file 'C:\\Temp\\ct-task'"), "Windows task launch passes a task file path");
  assert(!cmd.includes("Get-Content"), "Windows task launch does not expand task through PowerShell");
  assert(!cmd.includes("<location>") && !cmd.includes("whoami") && !cmd.includes("юникод"), "Windows task body is not embedded in the command");
}

console.log("\n--- POSIX task launch shape ---\n");
{
  const p = loadClaudeWithHost(baseHost({
    platform: () => "linux",
    fs: {
      fileExists: () => false,
      readFile: () => null,
      readJson: () => null,
      writeTempFile: () => "/tmp/ct-task",
      writeJson: () => true,
    },
    shell: {
      quoteFor: (value: unknown) => `'${String(value).replace(/'/g, "'\\''")}'`,
      quote: (value: unknown) => `'${String(value).replace(/'/g, "'\\''")}'`,
    },
  }));
  const cmd = p.buildLaunchCommand({ task: "line one\n<location>/repo</location>\n$HOME `whoami` --flag\nюникод" });
  assert(cmd.includes("--append-system-prompt-file '/tmp/ct-task'"), "POSIX task launch passes a task file path");
  assert(!cmd.includes("$(cat "), "POSIX task launch does not expand task into argv");
  assert(!cmd.includes("<location>") && !cmd.includes("whoami") && !cmd.includes("юникод"), "POSIX task body is absent from the command");
  const idle = p.buildLaunchCommand({ task: "identity", starterPrompt: "idle_wakeup" });
  assert(idle.includes("You are a newly started agent. Wait for instructions from the user or your orchestration parent."), "Claude JS renders idle starter");
  assert(!idle.includes("Execute the CodeTerm task provided"), "Claude JS idle starter is not task action");
  const teamText = "host-owned team bootstrap";
  const team = p.buildLaunchCommand({ task: "identity", starterPrompt: "team_bootstrap", starterPromptText: teamText });
  assert(team.includes(teamText), "Claude JS team launch uses the host-owned starter text");
  eq(p.starterPromptText?.("team_bootstrap"), undefined, "Claude JS does not duplicate the team starter");
}

console.log("\n--- parseUsage reset formats ---\n");

const now = new Date(2026, 5, 14, 15, 0, 0, 0).getTime();
const usage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 3:30pm (Europe/Kiev)",
    "Current week (all models) 64% usedResets Jun 14, 2026 at 4 PM (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(usage?.weekly_pct === 64, "parseUsage extracts weekly pct");
assert(usage?.weekly_resets_at_ms === new Date(2026, 5, 14, 16, 0, 0, 0).getTime(), "parseUsage accepts comma/year reset date");

const sameDayUsage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 3:30pm (Europe/Kiev)",
    "Current week (all models) 64% usedResets 4 PM (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(sameDayUsage?.weekly_resets_at_ms === new Date(2026, 5, 14, 16, 0, 0, 0).getTime(), "parseUsage accepts bare weekly reset time");

const claudeCurrentUsage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 3:39pm (Europe/Kiev)",
    "Current week (all models) 64% usedResets Jun 14, 7:59pm (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(claudeCurrentUsage?.weekly_resets_at_ms === new Date(2026, 5, 14, 19, 59, 0, 0).getTime(), "parseUsage accepts month-day comma time");

const incompleteSessionUsage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 8:10p (Europe/Kiev)",
    "Current week (all models) 55% usedResets Jun 14, 8pm (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(incompleteSessionUsage === null, "parseUsage rejects an incomplete current-session reset frame");

const completeSessionUsage = hostPlugin.parseUsage(
  [
    "Current session 100% usedResets 8:10pm (Europe/Kiev)",
    "Current week (all models) 55% usedResets Jun 14, 8pm (Europe/Kiev)",
  ].join("\n"),
  now,
);
assert(
  completeSessionUsage?.session_resets_at_ms === new Date(2026, 5, 14, 20, 10, 0, 0).getTime(),
  "parseUsage accepts the completed current-session reset frame",
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
