/**
 * Plugin contract tests for Claude — meta, statusOf, actions, detect.
 * Run: npx tsx plugins/provider-claude/tests/plugin.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import plugin from "../src/index";
import { claudeFolderTrustHighlightedOptionIndex, claudeFolderTrustPrompt } from "../src/folderTrust";

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

console.log("\n--- session model detection ---\n");
{
  const transcript = '{"type":"assistant","message":{"model":"claude-opus-5","content":[]}}';
  let fullReads = 0;
  let tailBytes = 0;
  const p = loadClaudeWithHost(baseHost({
    fs: {
      fileExists: () => true,
      readFile: () => { fullReads += 1; return transcript; },
      readFileTail: (_path: string, maxBytes: number) => {
        tailBytes = maxBytes;
        return transcript;
      },
      readJson: () => null,
      writeTempFile: () => "/tmp/ct-task",
      writeJson: () => true,
    },
  }));

  eq(
    p.detectSessionModel("/fixture/project", "session-model"),
    "Opus",
    "session model uses the exact session transcript",
  );
  eq(fullReads, 0, "session model avoids loading the full transcript");
  eq(tailBytes, 4 * 1024 * 1024, "session model reads a bounded 4 MiB transcript tail");
}

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
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "onboarding");
  const files = [
    "auto-environment-captured.txt",
    "auto-environment-not-now.txt",
    "auto-environment-boxed-curly.txt",
    "auto-environment-variant.txt",
  ];
  const cleanRow = (row: string) => row
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/^\s*[│┃║]\s*/, "")
    .replace(/\s*[│┃║]\s*$/, "")
    .trim();
  for (const file of files) {
    const screen = fs.readFileSync(path.join(dir, file), "utf8");
    const rows = screen.split(/\r?\n/).map(cleanRow);
    const optionRows = rows.flatMap((row) => {
      const match = /^\s*([❯>])?\s*(\d+)\.\s*(.*?)\s*$/.exec(row);
      return match ? [{ focused: Boolean(match[1]), label: match[3], value: match[2] }] : [];
    }).slice(-3);
    const first = rows.findIndex((row) => /^\s*([❯>])?\s*1\.\s/.test(row));
    const question = rows.slice(Math.max(0, first - 5), first).reverse().find((row) => row.endsWith("?"));
    const parsed = hostPlugin.parsePrompt(screen);
    const response = hostPlugin.launchOnboardingResponse(screen);
    assert(Boolean(response), `onboarding capability recognizes ${file}`);
    eq(parsed?.question, question, `pending prompt question comes from the dialog row in ${file}`);
    eq(parsed?.options?.map(([label, value]) => [label, value]), optionRows.map(({ label, value }) => [label, value]), `pending prompt choices preserve captured order in ${file}`);
    const focused = optionRows.findIndex((option) => option.focused);
    eq(response?.frameKey, `onboarding:focus-${focused}:target-1`, `stable onboarding identity tracks focus in ${file}`);
    eq(response?.step, focused === 1 ? "DQ==" : "ag==", `one next key moves only toward the safe choice in ${file}`);
  }

  const captured = fs.readFileSync(path.join(dir, files[0]), "utf8");
  const capturedQuestion = captured.split(/\r?\n/).find((row) => cleanRow(row).endsWith("?"));
  assert(Boolean(capturedQuestion), "captured dialog has a structural question row");
  const firstOption = captured.indexOf("1. Yes");
  const focus = captured.lastIndexOf("❯", firstOption);
  assert(firstOption >= 0 && focus >= 0, "captured default focus is on the first option");
  eq(hostPlugin.launchOnboardingResponse(captured.slice(0, focus) + captured.slice(focus + 1)), null, "ambiguous focus is left unanswered");
  eq(
    hostPlugin.launchOnboardingResponse(`${captured}\nstatus refreshed`)?.frameKey,
    hostPlugin.launchOnboardingResponse(captured)?.frameKey,
    "incidental status redraw retains the same choice/focus identity",
  );
  for (const file of ["auto-environment-no-space.txt", "auto-environment-empty-label.txt"]) {
    const malformed = fs.readFileSync(path.join(dir, file), "utf8");
    eq(hostPlugin.launchOnboardingResponse(malformed), null, `unsafe option syntax is refused in ${file}`);
    const rows = malformed.split(/\r?\n/).map(cleanRow);
    const recognizedOption = rows.findIndex((row) => /^\s*[❯>]?\s*\d+\.\s+\S/.test(row));
    const structuralQuestion = rows.slice(0, recognizedOption).reverse().find((row) => row.endsWith("?"));
    const parsed = hostPlugin.parsePrompt(malformed);
    eq(parsed?.question, structuralQuestion, `fallback prompt question follows the first recognized option in ${file}`);
    eq(parsed?.options?.map(([, value]) => value), ["2", "3"], `fallback options omit malformed first row in ${file}`);
  }
  eq(hostPlugin.launchOnboardingResponse(captured.replace(capturedQuestion!, "A different startup question?")), null, "unclassified startup dialog is left alone");
}

{
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "folder-trust");
  const cases = JSON.parse(fs.readFileSync(path.join(dir, "cases.json"), "utf8")) as Array<{ file: string; expect: string }>;
  const keys: Record<string, string[] | null> = {
    enter: ["DQ=="],
    "move-down": ["ag=="],
    "move-up": ["aw=="],
    hold: [],
    none: null,
  };
  assert(cases.length >= 12, "every declared folder-trust fixture is exercised");
  for (const c of cases) {
    const screen = fs.readFileSync(path.join(dir, c.file), "utf8");
    eq(hostPlugin.directoryTrustPromptResponse(screen), keys[c.expect], `folder trust ${c.file} -> ${c.expect}`);
  }

  const unnumbered = fs.readFileSync(path.join(dir, "quick-safety-no-first.txt"), "utf8");
  const parsed = hostPlugin.parsePrompt(unnumbered);
  const trustPrompt = hostPlugin.parseDirectoryTrustPrompt(unnumbered);
  assert(Boolean(parsed?.question.trim()), "folder trust exposes a parsed prompt question");
  eq(parsed?.options?.map(([, value]) => value), ["1", "2"], "folder trust options retain screen order");
  eq(trustPrompt, parsed, "dedicated trust parser exposes the same classified choices");
  eq(parsed?.options?.map(([label]) => label.match(/^[A-Za-z]+/)?.[0].toLowerCase()), ["no", "yes"], "folder trust options expose refusal before trust");
  eq(hostPlugin.buildPromptResponse(1, unnumbered), ["ag==", "DQ=="], "responding with option two moves off refusal before confirming");
  eq(hostPlugin.permissionPromptResponse("Bash", parsed, unnumbered), null, "directory trust is never a tool permission approval");

  const wrapped = fs.readFileSync(path.join(dir, "quick-safety-wrapped-header.txt"), "utf8");
  const wrappedPrompt = hostPlugin.parsePrompt(wrapped);
  assert(Boolean(wrappedPrompt?.question.trim()), "wrapped trust header remains one parsed question");
  eq(wrappedPrompt?.options?.map(([, value]) => value), ["1", "2"], "wrapped header keeps choices in screen order");

  const pathResidue = fs.readFileSync(path.join(dir, "quick-safety-path-residue.txt"), "utf8");
  const residuePrompt = hostPlugin.parsePrompt(pathResidue);
  assert(Boolean(residuePrompt?.question.trim()), "unrelated path residue does not hide a trust prompt");

  const pathInDialog = fs.readFileSync(path.join(dir, "quick-safety-path-in-dialog.txt"), "utf8");
  eq(hostPlugin.parsePrompt(pathInDialog), null, "a path within the dialog block is not parsed as the pathless layout");

  const trustMentionedBypass = fs.readFileSync(path.join(dir, "trust-mentioned-bypass-warning.txt"), "utf8");
  eq(claudeFolderTrustPrompt(trustMentionedBypass), null, "a bypass warning is not a folder-trust prompt");
  eq(claudeFolderTrustHighlightedOptionIndex(trustMentionedBypass), null, "a bypass warning has no folder-trust focus");
  const titleless = fs.readFileSync(path.join(dir, "trust-without-title.txt"), "utf8");
  eq(claudeFolderTrustPrompt(titleless), null, "an untitled folder trust prompt is refused");
  eq(claudeFolderTrustHighlightedOptionIndex(titleless), null, "an untitled prompt has no trusted focus");
  const updateDialogPath = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "../../provider-codex/tests/fixtures/directory-trust/codex-update-available.txt",
  );
  const updateDialog = fs.readFileSync(updateDialogPath, "utf8");
  eq(hostPlugin.directoryTrustPromptResponse(updateDialog), null, "Claude trust responder refuses the unrelated update menu");
  eq(claudeFolderTrustPrompt(updateDialog), null, "Claude folder-trust parser refuses the unrelated update menu");
  const underscoreTrust = fs.readFileSync(path.join(dir, "quick-safety-no-first.txt"), "utf8")
    .replace(/^([ \t]*Yes\b[^\r\n]*)\btrust\b/im, "$1_trust");
  eq(claudeFolderTrustPrompt(underscoreTrust), null, "underscore is part of the trust token");

  const numbered = fs.readFileSync(path.join(dir, "trust-numbered-focus-affirmative.txt"), "utf8");
  const numberedPrompt = hostPlugin.parsePrompt(numbered);
  eq(numberedPrompt?.options?.map(([, value]) => value), ["1", "2"], "numbered trust options remain parseable");
  eq(numberedPrompt?.options?.map(([label]) => label.match(/^[A-Za-z]+/)?.[0].toLowerCase()), ["yes", "no"], "numbered trust choices retain screen order");
  eq(hostPlugin.buildPromptResponse(0, numbered), ["DQ=="], "numbered trust choice confirms its focused affirmative row");
}

{
  const approval = [
    "Bash command",
    "",
    "  git status --short",
    "  Show working tree status",
    "",
    "Do you want to proceed?",
    "❯ 1. Yes",
    "  2. Yes, and don't ask again for git status commands in /repo",
    "  3. No, and tell Claude what to do differently (esc)",
    "",
    "Esc to cancel",
  ].join("\n");
  const parsed = hostPlugin.parsePrompt(approval);
  const answered = hostPlugin.permissionPromptResponse("Bash", parsed, approval);
  eq(answered?.optionIndex, 0, "tool approval selects the one-shot affirmative option");
  eq(answered?.steps, ["DQ=="], "tool approval confirms the highlighted option");
  assert(typeof answered?.rule === "string" && answered.rule.length > 0, "tool approval names the rule that matched");
  eq(hostPlugin.permissionPromptResponse("AskUserQuestion", parsed, approval), null, "clarifying question tool is never answered");
  eq(hostPlugin.permissionPromptResponse("ExitPlanMode", parsed, approval), null, "plan decision is never answered");
  eq(hostPlugin.permissionPromptResponse("Bash", parsed, "nothing on screen"), null, "a prompt no longer on screen is never answered");
  const declineless = { question: "Do you want to proceed?", options: [["Yes", "1"], ["Yes, always", "2"]] };
  eq(hostPlugin.permissionPromptResponse("Bash", declineless, approval), null, "a prompt without a decline path is never answered");
  const rustShape = { question: "Do you want to proceed?", options: [{ label: "Yes", value: "1" }, { label: "No", value: "2" }], multi_select: true };
  eq(hostPlugin.permissionPromptResponse("Bash", rustShape, approval), null, "multi-select pickers are never answered");
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

console.log("\n--- parseSessionDelta thinking ---\n");
{
  const rows = hostPlugin.parseSessionDelta([
    JSON.stringify({ type: "assistant", uuid: "think-a", message: { id: "msg_think", content: [{ type: "thinking", thinking: "Plan the edit", signature: "opaque-signature-a" }] } }),
    JSON.stringify({ type: "assistant", uuid: "think-b", message: { id: "msg_think", content: [{ type: "thinking", thinking: "", signature: "opaque-signature-b" }] } }),
    JSON.stringify({ type: "assistant", uuid: "think-c", message: { content: [{ type: "thinking", thinking: "Unlinked" }] } }),
  ].join("\n")).messages as Array<{ blocks: Array<{ kind: string; data: Record<string, unknown> }> }>;
  const blocks = rows.flatMap((row) => row.blocks);
  eq(blocks[0], { kind: "thinking", data: { thinking: "Plan the edit" } }, "thinking keeps its text");
  eq(blocks[1].data, { thinking: "" }, "a signature-only block carries no displayable text");
  eq(blocks[2].data, { thinking: "Unlinked" }, "unsigned thinking passes through unchanged");
  assert(!JSON.stringify(rows).includes("opaque-signature"), "no signature material crosses the provider boundary");
}

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

console.log("\n--- tool descriptions and proven flow ids ---\n");
{
  const bashInput = { command: "npm test -- --run", description: "Run unit tests" };
  const lines = [
    { type: "assistant", message: { id: "msg_parallel", content: [{ type: "tool_use", id: "tu-1", name: "Bash", input: bashInput }] } },
    { type: "assistant", message: { id: "msg_parallel", content: [{ type: "tool_use", id: "tu-2", name: "Read", input: { file_path: "src/a.ts" } }] } },
    { type: "assistant", message: { content: [{ type: "tool_use", id: "tu-3", name: "Bash", input: { command: "ls", description: "  " } }] } },
    { type: "assistant", message: { id: "msg_other", content: [{ type: "tool_use", id: "tu-4", name: "Grep", input: { pattern: "todo", description: "Find\nTODOs" } }] } },
  ].map((line) => JSON.stringify(line)).join("\n");
  const parsed = hostPlugin.parseSessionDelta(lines) as { messages: Array<{ blocks: Array<{ data: Record<string, unknown> }> }> };
  const data = parsed.messages.map((message) => message.blocks[0].data);
  eq(data[0].tool_description, "Run unit tests", "Claude description becomes the normalized description");
  eq(data[0].tool_summary, "npm test -- --run", "description never replaces the command summary");
  eq(data[0].input, bashInput, "exact Claude input is preserved");
  eq(data[0].tool_flow_id, "msg_parallel", "parallel calls carry their shared message id");
  eq(data[1].tool_flow_id, "msg_parallel", "a sibling call shares the same flow id");
  eq(data[1].tool_description, undefined, "a call without a description stays unknown");
  eq(data[2].tool_description, undefined, "a blank description stays unknown");
  eq(data[2].tool_flow_id, undefined, "a missing message id leaves the flow unknown");
  eq(data[3].tool_description, "Find TODOs", "descriptions collapse to one line");
  eq(data[3].tool_flow_id, "msg_other", "separate responses keep separate flows");
}

console.log("\n--- integration consent and exact-byte transactions ---\n");

console.log("\n--- Bash outcome facts and exact source identities ---\n");
{
  const parse = (entry: Record<string, unknown>) => hostPlugin.parseSessionDelta(JSON.stringify(entry)) as {
    messages: Array<{ blocks: Array<{ data: Record<string, unknown> }> }>;
  };
  const call = parse({
    type: "assistant",
    uuid: "assistant-outcome-exact",
    message: { id: "message-outcome", content: [{ type: "tool_use", id: "tool-outcome", name: "Bash", input: { command: "printf ok" } }] },
  });
  const result = parse({
    type: "user",
    uuid: "user-outcome",
    sourceToolAssistantUUID: "assistant-outcome-exact",
    toolUseResult: { returnCode: 0, durationMs: 1250, interrupted: false, backgroundTaskId: "background-outcome" },
    message: { role: "user", content: [{ type: "tool_result", tool_use_id: "tool-outcome", content: "ok", is_error: false }] },
  });
  const callData = call.messages[0].blocks[0].data;
  const resultData = result.messages[0].blocks[0].data;
  eq(callData.tool_call_source_id, "assistant-outcome-exact", "Claude Bash call preserves its source identity");
  eq(resultData.tool_call_source_id, "assistant-outcome-exact", "Claude result preserves the exact source identity");
  eq(resultData.tool_pairing, "exact", "Claude result marks exact source pairing");
  eq(resultData.tool_exit_code, 0, "Claude Bash preserves an explicit zero exit code");
  eq(resultData.tool_duration_ms, 1250, "Claude Bash preserves duration milliseconds");
  eq(resultData.tool_interrupted, false, "Claude Bash preserves explicit non-interruption");
  eq(resultData.tool_background_task_id, "background-outcome", "Claude Bash preserves a background task id");
  const interrupted = parse({
    type: "user",
    uuid: "user-interrupted-outcome",
    sourceToolAssistantUUID: "assistant-outcome-exact",
    toolUseResult: { returnCode: 130, durationMs: 600, interrupted: true },
    message: { role: "user", content: [{ type: "tool_result", tool_use_id: "tool-outcome", content: "stopped", is_error: true }] },
  }).messages[0].blocks[0].data;
  eq(interrupted.tool_exit_code, 130, "Claude Bash preserves a nonzero exit code");
  eq(interrupted.tool_interrupted, true, "Claude Bash normalizes interruption");

  const malformed = parse({
    type: "user",
    uuid: "user-malformed-outcome",
    sourceToolAssistantUUID: { stale: true },
    toolUseResult: { returnCode: "7", durationMs: -1, interrupted: "yes", backgroundTaskId: 17, unknown: "ignored" },
    message: { role: "user", content: [{ type: "tool_result", tool_use_id: "tool-outcome", content: "unknown" }] },
  }).messages[0].blocks[0].data;
  eq(malformed.tool_call_source_id, undefined, "malformed Claude source identity stays absent");
  eq(malformed.tool_pairing, "unavailable", "malformed Claude source identity cannot claim an exact pair");
  eq(malformed.tool_exit_code, undefined, "malformed Claude exit code stays absent");
  eq(malformed.tool_duration_ms, undefined, "malformed Claude duration stays absent");
  eq(malformed.tool_interrupted, undefined, "malformed Claude interruption stays absent");
  eq(malformed.tool_background_task_id, undefined, "malformed Claude background id stays absent");
}

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
  const handshakeText = "opaque-startup-handshake";
  const handshake = p.buildLaunchCommand({ task: "identity", starterPrompt: "startup_handshake", starterPromptText: handshakeText });
  assert(handshake.includes(handshakeText), "Claude JS startup launch uses the host-owned starter text");
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

console.log("\n--- versionless Claude alias catalog ---\n");
{
  const repoRoot = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
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
  eq(
    manifest.models.map((m) => m.displayName),
    ["Claude Opus", "Claude Sonnet", "Claude Haiku", "Claude Fable"],
    "cold catalog labels stay stable and versionless",
  );
  for (const m of manifest.models) {
    if (m.id === "fable") {
      // Do not advertise model availability until installed-CLI discovery proves it.
      assert(m.available !== true, "fable's static seed never claims availability ahead of CLI-version discovery");
    } else {
      assert(!("available" in m), `static seed availability is unknown for ${m.id}`);
    }
    assert(typeof m.description === "string" && m.description.length > 0, `${m.id} keeps a description`);
  }
}


// A marker-less hook that only resembles a CodeTerm command is user-owned: the
// installer preserves it beside its own entry, while exact installed forms migrate.
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
              // Near the shipped shape but not it: no auth header, so it fails
              // the migration fingerprint and stays unclassified.
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
  assert(result.ok === true, "install alongside a marker-less hook succeeds");
  const sessionStart = written?.hooks?.SessionStart ?? [];
  eq(sessionStart.length, 2, "an install preserves an unclassified hook and adds its own entry");
  const commands = sessionStart.flatMap((entry: any) => entry.hooks ?? []);
  eq(commands.length, 2, "both the foreign and owned SessionStart commands survive");
  assert(
    commands.some((command: any) => command._source && String(command._source).indexOf("codeterm-hook-") === 0),
    "the added hook carries a versioned ownership marker",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
