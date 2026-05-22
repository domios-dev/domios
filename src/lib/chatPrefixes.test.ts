/**
 * Tests for chatPrefixes prefix detection.
 * Run: npx tsx src/lib/chatPrefixes.test.ts
 */

import { detectChatPrefix } from "./chatPrefixes";

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

// Cross-pane prefix
{
  const r = detectChatPrefix("[from Torvalds pane=89186f68] hello world");
  eq(r?.origin, { kind: "pane", title: "Torvalds", paneIdShort: "89186f68" }, "matches cross-pane prefix");
  eq(r?.body, "hello world", "cross-pane body stripped");
}

// Pane title with spaces
{
  const r = detectChatPrefix("[from Main Dev pane=abc123de] msg");
  eq(r?.origin, { kind: "pane", title: "Main Dev", paneIdShort: "abc123de" }, "matches pane title with spaces");
  eq(r?.body, "msg", "pane with spaces body stripped");
}

// Telegram bridge without msg_id
{
  const r = detectChatPrefix("[Telegram: 12345] sup");
  eq(r?.origin, { kind: "telegram", userId: "12345", messageId: undefined }, "matches Telegram bridge");
  eq(r?.body, "sup", "telegram body stripped");
}

// Telegram bridge with msg_id
{
  const r = detectChatPrefix("[Telegram: 12345 msg_id=42] yo");
  eq(r?.origin, { kind: "telegram", userId: "12345", messageId: "42" }, "matches Telegram with msg_id");
  eq(r?.body, "yo", "telegram with msg_id body stripped");
}

// Mesh bridge
{
  const r = detectChatPrefix("[Mesh: peer-xyz] ping");
  eq(r?.origin, { kind: "mesh", peerId: "peer-xyz" }, "matches Mesh bridge");
  eq(r?.body, "ping", "mesh body stripped");
}

// Unknown service bridge
{
  const r = detectChatPrefix("[Slack: U123] hi");
  eq(r?.origin, { kind: "service", service: "Slack", userId: "U123" }, "falls back to generic service");
  eq(r?.body, "hi", "service body stripped");
}

// Plain messages — no prefix
{
  const r1 = detectChatPrefix("just a normal message");
  eq(r1, null, "returns null for plain user messages");

  const r2 = detectChatPrefix("[from foo no pane id]");
  eq(r2, null, "returns null for malformed prefix (no pane=)");
}

// Single slash command
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

// Command with stdout
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

// Multiple commands in one content
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

// Caveat stripped as noise
{
  const c =
    "<local-command-caveat>some claude meta</local-command-caveat><command-name>/foo</command-name>";
  const r = detectChatPrefix(c);
  assert(r?.origin.kind === "command", "caveat stripped: kind is command");
  if (r?.origin.kind === "command") {
    eq(r.origin.commands, [{ name: "/foo" }], "caveat stripped: only the command survives");
  }
}

// Summary
console.log();
if (failed === 0) {
  console.log(`✓ all ${passed} tests passed`);
} else {
  console.error(`✗ ${failed} test(s) failed (${passed} passed)`);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).process?.exit(1);
}
