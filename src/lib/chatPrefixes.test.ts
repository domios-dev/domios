import { detectChatPrefix, framePrefix, labelFor } from "./chatPrefixes";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`);
  }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

for (const [attrs, origin] of [
  [{ from: "tab", label: "Probe", tab: "41f7661c", model: "gpt-6.1-sol" }, { kind: "tab", title: "Probe", tabIdShort: "41f7661c", model: "gpt-6.1-sol" }],
  [{ from: "report", label: "Worker", tab: "abc" }, { kind: "tab", title: "Worker", tabIdShort: "abc", report: true }],
  [{ from: "telegram", user: "42", msg: "7" }, { kind: "telegram", userId: "42", messageId: "7" }],
  [{ from: "mesh", peer: "peer" }, { kind: "mesh", peerId: "peer" }],
  [{ from: "plugin", plugin: "hook" }, { kind: "plugin", pluginId: "hook" }],
  [{ from: "system", context: "blueprint:bp_7:result" }, { kind: "automation", source: "blueprint", name: "bp_7", detail: "result" }],
  [{ from: "system", context: "cron:nightly:completed" }, { kind: "automation", source: "cron", name: "nightly", detail: "completed" }],
  [{ from: "plugin_config", plugin: "p", name: "Plugin" }, { kind: "plugin_config", pluginId: "p", name: "Plugin" }],
  [{ from: "system_prompt" }, { kind: "system_prompt" }],
] as Array<[Record<string, string>, unknown]>) {
  eq(framePrefix(attrs, "opaque body"), { origin, body: "opaque body" }, `structured origin ${attrs.from}`);
}
eq(framePrefix({ from: "tab" }, "opaque"), null, "missing Tab identity stays unknown");
eq(labelFor({ kind: "system", context: "  SyStEm  " }), "System", "duplicate System context is omitted");
eq(detectChatPrefix('<domios from="tab" tab="a">body</domios>'), null, "raw strings are never routed by TS");

{
  const r1 = detectChatPrefix("just a normal message");
  eq(r1, null, "returns null for plain user messages");

  const r2 = detectChatPrefix("[codeterm from Edison tab=e8a0ac20 model opus] old");
  eq(r2, null, "old square codeterm prefix is plain text");

  const r3 = detectChatPrefix("from Main tab=12e450b4 model Sonnet old");
  eq(r3, null, "old bare Tab prefix is plain text");

  const r4 = detectChatPrefix("[Telegram: 12345] old");
  eq(r4, null, "old bridge prefix is plain text");

  const r5 = detectChatPrefix("[[ct:tab label=Main tab=12e450b4]]old");
  eq(r5, null, "old square ct marker is plain text");

  const r6 = detectChatPrefix("-=-codeterm:tab label=Main tab=12e450b4-=-old");
  eq(r6, null, "legacy route marker is ordinary content");
}

{
  const c =
    "<command-name>/model</command-name><command-message>model</command-message><command-args></command-args>";
  const r = detectChatPrefix(c);
  eq(
    r?.origin,
    { kind: "command", commands: [{ name: "/model" }] },
    "parses single slash command",
  );
  eq(r?.body, "", "command body is empty");
}

{
  const c =
    "<command-name>/add-dir</command-name><command-message>add-dir</command-message>" +
    "<command-args></command-args><local-command-stdout>Did not add a working directory.</local-command-stdout>";
  const r = detectChatPrefix(c);
  eq(
    r?.origin,
    { kind: "command", commands: [{ name: "/add-dir", stdout: "Did not add a working directory." }] },
    "parses command with stdout",
  );
}

{
  const c =
    "<command-name>/model</command-name><command-args></command-args>" +
    "<local-command-stdout>Set model to Sonnet 4.6 for this session</local-command-stdout>" +
    "<command-name>/model</command-name><command-args>opus</command-args>";
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "multiple commands: kind is command");
  if (r?.origin.kind === "command") {
    assert(r.origin.commands.length === 2, "multiple commands: count is 2");
    eq(
      r.origin.commands[0],
      { name: "/model", stdout: "Set model to Sonnet 4.6 for this session" },
      "multiple commands: first command",
    );
    eq(r.origin.commands[1], { name: "/model", args: "opus" }, "multiple commands: second command");
  }
}

{
  const c =
    "<local-command-caveat>some claude meta</local-command-caveat><command-name>/foo</command-name>";
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "caveat stripped: kind is command");
  if (r?.origin.kind === "command") {
    eq(r.origin.commands, [{ name: "/foo" }], "caveat stripped: only the command survives");
  }
}

{
  const c = `<local-command-caveat>Caveat: stuff</local-command-caveat>
/model
<local-command-stdout>Set model to Sonnet 4.6 for this session</local-command-stdout>`;
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "new format: kind is command");
  if (r?.origin.kind === "command") {
    eq(r.origin.commands.length, 1, "new format: one command");
    eq(
      r.origin.commands[0],
      { name: "/model", args: undefined, stdout: "Set model to Sonnet 4.6 for this session" },
      "new format: command parsed correctly",
    );
  }
  eq(r?.body, "", "new format: body is empty");
}

{
  const c = `<local-command-caveat>x</local-command-caveat>
/model
<local-command-stdout>Set model to Sonnet 4.6 for this session</local-command-stdout>
<local-command-caveat>y</local-command-caveat>
/add-dir
<local-command-stdout>Did not add a working directory.</local-command-stdout>`;
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "stacked cmds: kind is command");
  if (r?.origin.kind === "command") {
    assert(r.origin.commands.length === 2, "stacked cmds: count is 2");
    eq(r.origin.commands[0].name, "/model", "stacked cmds: first name");
    eq(r.origin.commands[1].name, "/add-dir", "stacked cmds: second name");
    assert(
      (r.origin.commands[0].stdout ?? "").includes("Sonnet"),
      "stacked cmds: first stdout contains Sonnet",
    );
    assert(
      (r.origin.commands[1].stdout ?? "").includes("Did not"),
      "stacked cmds: second stdout contains Did not",
    );
  }
}

{
  const c = `<local-command-stdout>output</local-command-stdout>
No response requested.
/foo`;
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "no-response noise: kind is command");
  if (r?.origin.kind === "command") {
    assert(
      r.origin.commands.some((cm) => cm.name === "/foo"),
      "no-response noise: /foo command survives",
    );
  }
}

// Orphan stdout (no /cmd line)
{
  const c = `<local-command-stdout>just output</local-command-stdout>`;
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "orphan stdout: kind is command");
  if (r?.origin.kind === "command") {
    eq(
      r.origin.commands,
      [{ name: "local-command-stdout", stdout: "just output" }],
      "orphan stdout: rendered with wrapper tag",
    );
  }
}

{
  const r = detectChatPrefix("/home/foo/bar is a path");
  eq(r, null, "plain /path message returns null (no tags, not a cmd-only line)");
}

console.log(`${passed} passed, ${failed} failed`);
if (failed) process.exit(1);
