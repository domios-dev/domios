/**
 * Tests for chatPrefixes prefix detection.
 * Run: npx tsx src/lib/chatPrefixes.test.ts
 */

import { detectChatPrefix, splitChatPrefixedMessages } from "./chatPrefixes";

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

console.log("detectChatPrefix");

{
  const r = detectChatPrefix("-=-codeterm:tab label=Main tab=12e450b4 model=Sonnet-=-Hey");
  eq(
    r?.origin,
    { kind: "tab", title: "Main", tabIdShort: "12e450b4", model: "Sonnet" },
    "ct Tab marker parses",
  );
  eq(r?.body, "Hey", "ct Tab marker body stripped");
}

{
  const r = detectChatPrefix(
    "-=-codeterm:tab label=Main tab=12e450b4-=-Hey\n[to reply, run: codeterm send \"YOUR REPLY\" --tab 12e450b4]",
  );
  eq(r?.body, "Hey", "reply routing hint stays hidden from chat body");
}

{
  const r = detectChatPrefix(
    "-=-codeterm:tab label=Main tab=12e450b4-=-Hey\n-=-codeterm:reply tab=12e450b4-=-",
  );
  eq(r?.body, "Hey", "legacy reply routing hint stays hidden from chat body");
}

{
  const r = detectChatPrefix("-=-codeterm:tab label=CURRENT%20TAB tab=915fae8e model=codex-=-Hey self");
  eq(
    r?.origin,
    { kind: "tab", title: "CURRENT TAB", tabIdShort: "915fae8e", model: "codex" },
    "ct current-tab marker keeps Tab id for render-time name resolution",
  );
  eq(r?.body, "Hey self", "ct current-tab marker body stripped");
}

{
  const r = detectChatPrefix("-=-codeterm:report from=Worker tab=abc12345 model=gpt-5.5-=-done");
  eq(
    r?.origin,
    { kind: "tab", title: "Worker", tabIdShort: "abc12345", model: "gpt-5.5", report: true },
    "ct report marker parses",
  );
  eq(r?.body, "done", "ct report marker body stripped");
}

{
  const r = detectChatPrefix("-=-codeterm:telegram user=12345 msg=42-=-yo");
  eq(r?.origin, { kind: "telegram", userId: "12345", messageId: "42" }, "ct telegram marker parses");
  eq(r?.body, "yo", "ct telegram marker body stripped");
}

{
  const r = detectChatPrefix("-=-codeterm:pebble source=Pebble-=-yo");
  eq(r?.origin, { kind: "pebble" }, "ct Pebble marker parses");
  eq(r?.body, "yo", "ct Pebble marker body stripped");
}

{
  const r = detectChatPrefix("-=-codeterm:pebble source=Pebble plugin=pebble-=-yo");
  eq(r?.origin, { kind: "plugin", pluginId: "pebble", name: "Pebble" }, "plugin marker keeps its identity");
  eq(r?.body, "yo", "plugin marker body stripped");
  assert(r?.origin.kind === "plugin" && r.origin.iconSvg === undefined, "plugin marker carries no inline icon payload");
}

{
  const r = detectChatPrefix("-=-codeterm:system_prompt-=-# Shell rules\n\nUse tools carefully.");
  eq(r?.origin, { kind: "system_prompt" }, "ct system_prompt marker parses");
  eq(r?.body, "# Shell rules\n\nUse tools carefully.", "ct system_prompt marker body stripped");
}

{
  const r = detectChatPrefix("-=-codeterm:system context=plan-=-Reconcile the active Plan.");
  eq(r?.origin, { kind: "system", context: "plan" }, "ct system marker is not a Tab origin");
  eq(r?.body, "Reconcile the active Plan.", "ct system marker body stripped");
}

{
  const r = detectChatPrefix(
    "-=-codeterm:reminder from=Main tab=e8a0ac20 model=opus set_at=1000 fire_at=2000 id=dmsg-7-=-water the plants",
  );
  eq(
    r?.origin,
    {
      kind: "reminder",
      whoSet: "Main",
      tab: "e8a0ac20",
      model: "opus",
      setAt: "1000",
      fireAt: "2000",
      id: "dmsg-7",
    },
    "ct reminder marker parses",
  );
  eq(r?.body, "water the plants", "ct reminder marker body stripped");
}

{
  const r = detectChatPrefix("-=-codeterm:reminder set_at=5 fire_at=9-=-ping");
  eq(r?.origin, { kind: "reminder", setAt: "5", fireAt: "9" }, "ct reminder marker without creator parses");
  eq(r?.body, "ping", "ct reminder marker (no creator) body stripped");
}

{
  const r = detectChatPrefix(
    "-=-codeterm:reminder id=series every_ms=600000 next_at=700000 skipped=2-=-tick",
  );
  eq(
    r?.origin,
    { kind: "reminder", id: "series", everyMs: "600000", nextAt: "700000", skippedTicks: "2" },
    "recurring reminder metadata round-trips through its marker",
  );
  eq(r?.body, "tick", "recurring reminder marker keeps its payload");
}

{
  const r = splitChatPrefixedMessages(
    "-=-codeterm:tab label=Main tab=12e450b4 model=Sonnet-=-Hey" +
    "-=-codeterm:tab label=Claude tab=e8a0ac20-=-Rollo online",
  );
  assert(r?.length === 2, "splits adjacent ct Tab markers");
  eq(r?.[0].body, "Hey", "adjacent ct split first body");
  eq(r?.[1].origin, { kind: "tab", title: "Claude", tabIdShort: "e8a0ac20" }, "adjacent ct split second origin");
}

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

console.log();
if (failed === 0) {
  console.log(`✓ all ${passed} tests passed`);
} else {
  console.error(`✗ ${failed} test(s) failed (${passed} passed)`);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).process?.exit(1);
}
