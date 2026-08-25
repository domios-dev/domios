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

// The bundled manifest IS the shipped alias seed — tests read it from disk so
// a drifted copy here can never mask a stale plugin.json.
const manifestSeed = JSON.parse(fs.readFileSync(
  path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"),
  "utf8",
)) as { models: Array<{ id: string; displayName: string; description?: string }> };

const baseHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify(manifestSeed),
  homeDir: () => "/tmp",
  path: { equal: () => true },
  platform: () => "linux",
  unixNowMs: () => 0,
  envGet: () => null,
  exec: () => JSON.stringify({ error: "not configured" }),
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

console.log("\n--- discoverModels: genuine discovery only ---\n");

{
  const p = loadClaudeWithHost(baseHost({
    envGet: (name: string) => name === "ANTHROPIC_API_KEY" ? "test-key" : null,
    fetch: () => JSON.stringify({
      status: 200,
      body: JSON.stringify({
        data: [
          { id: "claude-opus-5", display_name: "Claude Opus 5" },
          { id: "claude-sonnet-5", display_name: "Claude Sonnet 5" },
          { id: "claude-haiku-4-5", display_name: "Claude Haiku 4.5" },
          { id: "claude-fable-5", display_name: "Claude Fable 5" },
        ],
      }),
    }),
  }));
  eq(
    p.discoverModels(),
    [
      { id: "opus", displayName: "Claude Opus 5 (1M context)", description: "Best for everyday, complex tasks.", available: true },
      { id: "sonnet", displayName: "Claude Sonnet 5", description: "Efficient for routine tasks.", available: true },
      { id: "haiku", displayName: "Claude Haiku 4.5", description: "Fastest for quick answers.", available: true },
      { id: "fable", displayName: "Claude Fable 5", description: "Most capable for your hardest and longest-running tasks.", available: true },
    ],
    "genuine /v1/models discovery maps concrete ids onto the same stable aliases with live availability",
  );
}

{
  const p = loadClaudeWithHost(baseHost({
    envGet: (name: string) => name === "ANTHROPIC_API_KEY" ? "test-key" : null,
    fetch: () => JSON.stringify({
      status: 200,
      body: JSON.stringify({ data: [{ id: "claude-fable-5", display_name: "Claude Fable 5" }] }),
    }),
  }));
  const partial = p.discoverModels() as Array<{ id: string; displayName: string; available: boolean }>;
  eq(partial.map((m) => m.id), ["opus", "sonnet", "haiku", "fable"], "partial discovery keeps alias ids stable");
  eq(partial.map((m) => m.available), [false, false, false, true], "partial discovery publishes account availability");
  eq(
    partial.map((m) => m.displayName),
    manifestSeed.models.map((m) => m.displayName),
    "aliases missing from the account keep their current manifest labels",
  );
}

{
  const p = loadClaudeWithHost(baseHost({
    fetch: () => { throw new Error("network must not be touched without credentials"); },
  }));
  eq(p.discoverModels(), null, "discoverModels returns null when both API keys are absent (fetch forbidden)");
}

for (const [label, fetchImpl] of [
  ["fetch failure", () => { throw new Error("offline"); }],
  ["non-200 response", () => JSON.stringify({ status: 503, body: "overloaded" })],
  ["malformed body", () => JSON.stringify({ status: 200, body: "not json" })],
  ["empty usable list", () => JSON.stringify({ status: 200, body: JSON.stringify({ data: [{ id: "  " }] }) })],
] as Array<[string, () => string]>) {
  const p = loadClaudeWithHost(baseHost({
    envGet: (name: string) => name === "CLAUDE_API_KEY" ? "test-key" : null,
    fetch: fetchImpl,
  }));
  eq(p.discoverModels(), null, `discoverModels returns null on ${label} — never the manifest fallback`);
}

console.log("\n--- integration consent and exact-byte transactions ---\n");

for (const [name, fixture] of [
  ["malformed", "{not-json"],
  ["non-object", "[]"],
  ["empty", ""],
] as const) {
  const settingsPath = "/tmp/.claude/settings.json";
  const files = new Map<string, string>([[settingsPath, fixture]]);
  let writes = 0;
  const p = loadClaudeWithHost(baseHost({
    expandHome: () => settingsPath,
    fileExists: (path: string) => files.has(path),
    removeFile: (path: string) => files.delete(path),
    fs: {
      readFile: (path: string) => files.get(path) ?? null,
      readJson: () => null,
      writeFile: () => { writes += 1; return true; },
      writeJson: () => { writes += 1; return true; },
    },
  }));
  const result = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(result.ok === false, `${name} Claude settings JSON is rejected`);
  eq(writes, 0, `${name} rejection has no side effects`);
  eq(files.get(settingsPath), fixture, `${name} Claude input bytes remain unchanged`);
}

for (const [name, fixture] of [
  ["malformed", "{not-json"],
  ["non-object", "[]"],
  ["malformed hook array", '{"hooks":{"Stop":{}}}'],
] as const) {
  const settingsPath = "/tmp/.claude/settings.json";
  const files = new Map<string, string>([[settingsPath, fixture]]);
  let writes = 0;
  const p = loadClaudeWithHost(baseHost({
    expandHome: () => settingsPath,
    fileExists: (path: string) => files.has(path),
    removeFile: (path: string) => files.delete(path),
    fs: {
      readFile: (path: string) => files.get(path) ?? null,
      readJson: () => null,
      writeFile: (path: string, value: string) => { writes += 1; files.set(path, value); return true; },
      writeJson: (path: string, value: unknown) => { writes += 1; files.set(path, JSON.stringify(value)); return true; },
    },
  }));
  const result = p.uninstallIntegration();
  assert(result.ok === false, `${name} Claude uninstall input is rejected`);
  eq(writes, 0, `${name} Claude uninstall rejection has no side effects`);
  eq(files.get(settingsPath), fixture, `${name} Claude uninstall bytes remain unchanged`);
}

{
  const settingsPath = "/tmp/.claude/settings.json";
  const original = '{\n  "foreign": true,\n  "hooks": {"Stop": []}\n}\n';
  const files = new Map<string, string>([[settingsPath, original]]);
  const host = baseHost({
    expandHome: () => settingsPath,
    fileExists: (path: string) => files.has(path),
    removeFile: (path: string) => files.delete(path),
    fs: {
      readFile: (path: string) => files.get(path) ?? null,
      readJson: (path: string) => {
        const value = files.get(path);
        return value === undefined ? null : JSON.parse(value);
      },
      writeFile: (path: string, value: string) => { files.set(path, value); return true; },
      writeJson: (path: string, value: unknown) => { files.set(path, JSON.stringify(value)); return true; },
    },
  });
  const p = loadClaudeWithHost(host);
  const denied = p.installIntegration(7686, { consent: false });
  assert(denied.ok === false && files.size === 1, "Claude integration writes nothing before consent");
  assert(p.installIntegration(7686, { consent: true, precompact_memory_export: false }).ok === true, "Claude consented install succeeds");
  eq(files.get(settingsPath + ".codeterm-backup"), original, "Claude backup preserves exact original bytes");
  const installed = JSON.parse(files.get(settingsPath) || "{}");
  assert(installed.foreign === true, "Claude install preserves foreign settings");
  assert(!installed.hooks.PreCompact, "Claude PreCompact export remains separately opt-in");
  assert(p.restoreIntegration().ok === true, "Claude restore succeeds");
  eq(files.get(settingsPath), original, "Claude restore reproduces exact original bytes");
}

console.log("--- permission_request ingestion ---");
{
  const settingsPath = "/tmp/.claude/settings.json";
  const files = new Map<string, string>([[settingsPath, "{}"]]);
  const p = loadClaudeWithHost(baseHost({
    expandHome: () => settingsPath,
    fileExists: (path: string) => files.has(path),
    removeFile: (path: string) => files.delete(path),
    fs: {
      readFile: (path: string) => files.get(path) ?? null,
      readJson: (path: string) => {
        const value = files.get(path);
        return value === undefined ? null : JSON.parse(value);
      },
      writeFile: (path: string, value: string) => { files.set(path, value); return true; },
      writeJson: (path: string, value: unknown) => { files.set(path, JSON.stringify(value)); return true; },
    },
  }));
  assert(p.installIntegration(7686, { consent: true, precompact_memory_export: false }).ok === true, "Claude install for permission_request succeeds");
  const installed = JSON.parse(files.get(settingsPath) || "{}");
  for (const key of ["PermissionRequest", "Notification"]) {
    const cmd = installed.hooks?.[key]?.[0]?.hooks?.[0]?.command as string | undefined;
    assert(typeof cmd === "string", `${key} hook is installed by the shipped plugin`);
    assert((cmd as string).indexOf("--data-binary @-") !== -1, `${key} forwards the stdin payload`);
    assert((cmd as string).indexOf("-d '{}'") === -1, `${key} never posts an empty body`);
    assert((cmd as string).indexOf("/hook/permission") !== -1, `${key} targets the existing permission route`);
    assert((cmd as string).indexOf("CODETERM_SESSION_BINDING_NONCE") !== -1, `${key} carries the binding nonce`);
  }
  eq(installed.hooks?.PermissionRequest?.[0]?.matcher, "*", "PermissionRequest fires per tool");
  eq(installed.hooks?.Notification?.[0]?.matcher, "permission_prompt", "Notification keeps the vendor matcher");

  const status = p.checkIntegration() as Record<string, unknown>;
  assert(status.permission_request_hook === true, "checkIntegration reports the PermissionRequest hook");
  assert(status.hooks_installed === true, "hooks_installed requires the new hook too");

  assert(p.uninstallIntegration().ok === true, "Claude uninstall succeeds");
  const cleaned = JSON.parse(files.get(settingsPath) || "{}");
  eq((cleaned.hooks?.PermissionRequest || []).length, 0, "uninstall removes the PermissionRequest hook");
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
  const silent = p.buildLaunchCommand({ task: "identity", starterPrompt: "no_starter" });
  assert(silent.includes("--append-system-prompt-file '/tmp/ct-task'"), "Claude JS silent launch keeps system instructions");
  assert(!silent.includes(" -- "), "Claude JS silent launch has no positional starter turn");
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

console.log("\n--- quota-free usage fetch ---\n");
{
  let request: Record<string, unknown> | null = null;
  const p = loadClaudeWithHost(baseHost({
    fetch: (raw: string) => {
      request = JSON.parse(raw);
      return JSON.stringify({
        status: 200,
        body: JSON.stringify({
          five_hour: { utilization: 17, resets_at: "2026-08-01T08:39:59.000Z" },
          seven_day: { utilization: 92, resets_at: "2026-08-02T16:59:59.000Z" },
        }),
      });
    },
    fs: {
      ...baseHost({}).fs,
      readFile: () => JSON.stringify({ claudeAiOauth: { accessToken: "secret-token" } }),
    },
  }));
  const raw = p.fetchUsage();
  const fetched = p.parseUsage(raw, now);
  eq(request?.url, "https://api.anthropic.com/api/oauth/usage", "fetchUsage uses the provider OAuth meter endpoint");
  eq(fetched?.session_pct, 17, "fetchUsage maps the five-hour window");
  eq(fetched?.weekly_pct, 92, "fetchUsage maps the seven-day window");
  eq(fetched?.weekly_resets_at_ms, Date.parse("2026-08-02T16:59:59.000Z"), "fetchUsage maps ISO reset time");
  assert(!raw.includes("secret-token"), "fetchUsage output never persists credentials");
}

{
  let execRequest: Record<string, unknown> | null = null;
  let fetchRequest: Record<string, unknown> | null = null;
  const p = loadClaudeWithHost(baseHost({
    platform: () => "macos",
    envGet: (name: string) => name === "USER" ? "mac-user" : null,
    exec: (raw: string) => {
      execRequest = JSON.parse(raw);
      return JSON.stringify({
        code: 0,
        stdout: JSON.stringify({ claudeAiOauth: { accessToken: "keychain-token" } }),
      });
    },
    fetch: (raw: string) => {
      fetchRequest = JSON.parse(raw);
      return JSON.stringify({
        status: 200,
        body: JSON.stringify({ five_hour: { utilization: 8 }, seven_day: { utilization: 7 } }),
      });
    },
    fs: {
      ...baseHost({}).fs,
      readFile: () => JSON.stringify({ claudeAiOauth: { accessToken: "stale-file-token" } }),
    },
  }));
  const raw = p.fetchUsage();
  eq(execRequest?.bin, "security", "fetchUsage reads current macOS Claude credentials from Keychain");
  assert(
    Array.isArray(execRequest?.args) && execRequest.args.includes("Claude Code-credentials"),
    "fetchUsage requests Claude's scoped Keychain service",
  );
  eq(
    (fetchRequest?.headers as Record<string, string>)?.Authorization,
    "Bearer keychain-token",
    "macOS Keychain credentials take precedence over a stale legacy file",
  );
  assert(!raw.includes("keychain-token"), "Keychain credentials never persist in usage output");
}

for (const [label, response, expected] of [
  ["network timeout", { error: "request failed: operation timed out" }, "operation timed out"],
  ["HTTP failure", { status: 503, body: "unavailable" }, "HTTP 503"],
] as const) {
  const p = loadClaudeWithHost(baseHost({
    fetch: () => JSON.stringify(response),
    fs: {
      ...baseHost({}).fs,
      readFile: () => JSON.stringify({ claudeAiOauth: { accessToken: "secret-token" } }),
    },
  }));
  let message = "";
  try { p.fetchUsage(); } catch (error) { message = String(error); }
  assert(message.includes(expected), `fetchUsage reports ${label} without collapsing it to null`);
  assert(!message.includes("secret-token"), `${label} diagnostics omit credentials`);
}

console.log("\n--- claude_alias_catalog_matches_model_picker_r3 ---\n");
{
  const repoRoot = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
  const picker = JSON.parse(fs.readFileSync(
    path.join(repoRoot, "testbench", "snapshots", "claude", "model-picker-r3", "expected.json"),
    "utf8",
  )) as { expected: { options: string[] } };
  const manifest = JSON.parse(fs.readFileSync(
    path.join(repoRoot, "plugins", "provider-claude", "plugin.json"),
    "utf8",
  )) as { models: Array<{ id: string; displayName: string; description?: string; available?: boolean }>; defaultModel?: string };

  eq(
    manifest.models.map((m) => m.id),
    ["opus", "sonnet", "haiku", "fable"],
    "stable alias ids stay opus,sonnet,haiku,fable in order",
  );
  eq(manifest.defaultModel, "sonnet", "default alias remains sonnet");

  for (const family of ["Opus", "Sonnet", "Haiku", "Fable"]) {
    const option = picker.expected.options.find((o) => new RegExp(`${family}\\s+\\d`).test(o));
    assert(!!option, `picker fixture has a ${family} row`);
    const version = new RegExp(`${family}\\s+(\\d+(?:\\.\\d+)?)`).exec(option!)?.[1];
    assert(!!version, `picker fixture carries a ${family} version`);
    const alias = manifest.models.find((m) => m.displayName.includes(family));
    assert(!!alias, `manifest has a ${family} alias`);
    assert(
      alias!.displayName.includes(`${family} ${version}`),
      `manifest ${family} label matches captured picker version`,
      `expected label containing "${family} ${version}", got "${alias!.displayName}"`,
    );
  }
  const opus = manifest.models.find((m) => m.id === "opus");
  assert(
    /1M context/i.test(opus?.displayName ?? ""),
    "opus label carries the 1M-context marker from the captured picker",
  );
  for (const m of manifest.models) {
    assert(
      !("available" in m),
      `static seed availability is unknown for ${m.id}`,
      `expected "available" absent, got ${JSON.stringify(m.available)}`,
    );
    assert(typeof m.description === "string" && m.description.length > 0, `${m.id} keeps a description`);
  }
}


// A previously-installed hook whose command uses $CODETERM_API_URL (no literal
// host:port) has to be recognised as ours, or every install appends a duplicate
// instead of replacing it — which is how one machine accumulated ten copies of
// every hook and fired ten POSTs per session start.
{
  const settingsPath = "/home/test/.claude/settings.json";
  const legacy = {
    hooks: {
      SessionStart: [
        {
          matcher: "",
          hooks: [
            {
              type: "command",
              // The shipped shape: env-var URL, no port literal, no _source marker.
              command:
                'if [ -n "${CODETERM_API_URL:-}" ]; then curl -s -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/session_start"; fi',
              timeout: 5000,
            },
          ],
        },
      ],
    },
  };
  let written: any = null;
  const p = loadClaudeWithHost(baseHost({
    expandHome: () => settingsPath,
    fileExists: () => true,
    fs: {
      readFile: () => JSON.stringify(legacy),
      readJson: () => JSON.parse(JSON.stringify(legacy)),
      writeFile: () => true,
      writeJson: (_path: string, value: unknown) => { written = value; return true; },
    },
  }));

  const result = p.installIntegration(7686, { consent: true, precompact_memory_export: false });
  assert(result.ok === true, "install over a marker-less hook succeeds");
  const sessionStart = written?.hooks?.SessionStart ?? [];
  eq(sessionStart.length, 1, "an install replaces the previous hook instead of appending a duplicate");
  const commands = sessionStart.flatMap((entry: any) => entry.hooks ?? []);
  eq(commands.length, 1, "exactly one SessionStart command survives");
  assert(
    commands[0]._source && String(commands[0]._source).indexOf("codeterm-hook") === 0,
    "the surviving hook carries the marker so the next check recognises it",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
