// Run: npx tsx packages/chat-core/src/sessionReset.test.ts
import { lastSessionResetIndex, trimAfterSessionReset } from "./sessionReset";
import type { NormalizedChatMessage } from "./types";

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

function msg(id: string, type: NormalizedChatMessage["type"], content: string): NormalizedChatMessage {
  return { id, type, content };
}

{
  const messages = [
    msg("u1", "user", "before"),
    msg("a1", "assistant", "reply"),
    msg("u2", "user", "/clear"),
    msg("a2", "assistant", "Cleared."),
  ];
  eq(lastSessionResetIndex(messages), 2, "finds last clear command");
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["u2", "a2"], "trims history before clear");
}

{
  const messages = [
    msg("u1", "user", "/clear"),
    msg("a1", "assistant", "Cleared."),
    msg("u2", "user", "/new"),
    msg("a2", "assistant", "Ready."),
  ];
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["u2", "a2"], "latest reset wins");
}

{
  const messages = [
    msg("u1", "user", "please type /clear literally"),
    msg("a1", "assistant", "/clear"),
  ];
  eq(lastSessionResetIndex(messages), -1, "only exact user slash commands reset");
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["u1", "a1"], "non-command text is preserved");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
