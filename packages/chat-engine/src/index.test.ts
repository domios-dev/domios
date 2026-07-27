import type { WatcherTickInput } from "@codeterm/plugin-sdk";
import { assembleChat, assembleMachine, type EngineMessage } from "./index";

let passed = 0;
let failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

function msg(role: EngineMessage["role"], content: string): EngineMessage {
  return { role, content };
}

// ── assembleChat: absent window ⇒ identity
{
  const history = [
    msg("system", "sys"),
    msg("user", "u1"),
    msg("assistant", "a1"),
    msg("user", "u2"),
  ];
  eq(assembleChat(history), history, "absent window returns full history unchanged");
}

// ── assembleChat: window without maxMessages ⇒ identity
{
  const history = [msg("user", "u1"), msg("assistant", "a1")];
  eq(assembleChat(history, {}), history, "window without maxMessages returns full history");
}

// ── assembleChat: top policy keeps leading system + last N non-system
{
  const history = [
    msg("system", "charter"),
    msg("user", "u1"),
    msg("assistant", "a1"),
    msg("user", "u2"),
    msg("assistant", "a2"),
    msg("user", "u3"),
    msg("assistant", "a3"),
  ];
  eq(
    assembleChat(history, { maxMessages: 2, policy: "top" }),
    [msg("system", "charter"), msg("user", "u3"), msg("assistant", "a3")],
    "top policy: system kept + last 2 non-system messages",
  );
}

// ── assembleChat: N larger than non-system history keeps all non-system
{
  const history = [msg("system", "sys"), msg("user", "u1"), msg("assistant", "a1")];
  eq(
    assembleChat(history, { maxMessages: 10 }),
    history,
    "maxMessages larger than history keeps everything",
  );
}

// ── assembleChat: no leading system message — tail only
{
  const history = [
    msg("user", "u1"),
    msg("assistant", "a1"),
    msg("user", "u2"),
    msg("assistant", "a2"),
  ];
  eq(
    assembleChat(history, { maxMessages: 2 }),
    [msg("user", "u2"), msg("assistant", "a2")],
    "no system message: only tail of N non-system",
  );
}

// ── assembleChat: empty history
{
  eq(assembleChat([]), [], "empty history stays empty");
  eq(assembleChat([], { maxMessages: 3 }), [], "empty history with window stays empty");
}

// ── assembleChat: mid-history system messages are dropped from the tail
{
  const history = [msg("user", "u1"), msg("system", "mid"), msg("assistant", "a1")];
  eq(
    assembleChat(history, { maxMessages: 2 }),
    [msg("user", "u1"), msg("assistant", "a1")],
    "mid-history system never enters assembled output",
  );
}

// ── assembleChat: leading, mid, and trailing system messages
{
  const history = [
    msg("system", "leading"),
    msg("user", "u1"),
    msg("system", "mid"),
    msg("assistant", "a1"),
    msg("user", "u2"),
    msg("system", "trailing"),
    msg("assistant", "a2"),
  ];
  eq(
    assembleChat(history, { maxMessages: 2 }),
    [msg("system", "leading"), msg("user", "u2"), msg("assistant", "a2")],
    "only leading system survives; mid/trailing system dropped from tail slots",
  );
}

// ── assembleChat: trailing system without leading system is dropped entirely
{
  const history = [msg("user", "u1"), msg("assistant", "a1"), msg("system", "trailing")];
  eq(
    assembleChat(history, { maxMessages: 3 }),
    [msg("user", "u1"), msg("assistant", "a1")],
    "trailing system dropped when not at index 0",
  );
}

// ── assembleMachine: exactly two messages
{
  const charter = "Observe orchestration health.";
  const state = { tick: 1, note: "prior" };
  const input: WatcherTickInput = {
    tick: 2,
    nowMs: 1_700_000_000_000,
    state,
    observations: { tabs: [] },
  };
  const out = assembleMachine(charter, state, input);
  eq(out.length, 2, "machine assembly returns exactly two messages");
  eq(out[0]?.role, "system", "first message is system");
  eq(out[1]?.role, "user", "second message is user");
}

// ── assembleMachine: charter + verdict contract in system
{
  const charter = "Watch the workers.";
  const out = assembleMachine(charter, {}, { query: "status?" });
  const system = out[0]?.content ?? "";
  eq(system.startsWith(charter), true, "system content starts with charter");
  eq(system.includes('"status": "ok" | "attention" | "stalled"'), true, "system includes verdict contract");
  eq(system.includes('"actions"'), true, "system includes actions schema");
}

// ── assembleMachine: state + input serialized in user message
{
  const state = { count: 3 };
  const input = { query: "what next?" };
  const out = assembleMachine("c", state, input);
  eq(out[1]?.content, JSON.stringify({ state, input }), "user message is JSON of state and input");
}

// ── assembleMachine: prior transcript never leaks
{
  const prior: EngineMessage[] = [
    msg("system", "old"),
    msg("user", "old user"),
    msg("assistant", "old assistant"),
  ];
  const out = assembleMachine("fresh charter", { v: 1 }, { tick: 1, nowMs: 0, state: { v: 1 } });
  for (const old of prior) {
    const leaked = out.some((m) => m.content === old.content);
    eq(leaked, false, `prior transcript does not leak: ${old.role}`);
  }
}

// ── assembleMachine: interactive query input shape
{
  const out = assembleMachine("charter", { s: true }, { query: "hello" });
  const parsed = JSON.parse(out[1]?.content ?? "{}") as { state: unknown; input: { query: string } };
  eq(parsed.input.query, "hello", "query input shape preserved in user JSON");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
