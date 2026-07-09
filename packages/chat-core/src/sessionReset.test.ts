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

{
  const messages = [
    msg("u1", "user", "some work"),
    msg("a1", "assistant", "reply"),
    msg("lc1", "local_command", "/new"),
    msg("a2", "assistant", "Session reset."),
  ];
  eq(lastSessionResetIndex(messages), 2, "local_command /new recognized as reset marker");
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["lc1", "a2"], "trims history before local_command /new");
}

// local_command with bare name (no leading slash) — some providers emit "new" not "/new"
{
  const messages = [
    msg("u1", "user", "some work"),
    msg("lc1", "local_command", "new"),
    msg("a1", "assistant", "Ready."),
  ];
  eq(lastSessionResetIndex(messages), 1, "local_command bare 'new' recognized as reset marker");
}

{
  const messages = [
    msg("u1", "user", "hello"),
    msg("lc1", "local_command", "/clear"),
    msg("a1", "assistant", "Cleared."),
    msg("lc2", "local_command", "/new"),
    msg("a2", "assistant", "Ready."),
  ];
  eq(lastSessionResetIndex(messages), 3, "latest local_command reset wins over earlier one");
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["lc2", "a2"], "trims to latest local_command /new");
}

{
  const messages = [
    msg("u1", "user", "/clear"),
    msg("a1", "assistant", "Cleared."),
    msg("lc1", "local_command", "/new"),
    msg("a2", "assistant", "Ready."),
  ];
  eq(lastSessionResetIndex(messages), 2, "local_command /new after user /clear is the last reset");
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["lc1", "a2"], "latest reset (local_command) trims correctly");
}

{
  const messages = [
    msg("u1", "user", "hello"),
    msg("lc1", "local_command", "compact"),
    msg("a1", "assistant", "Compacted."),
  ];
  eq(lastSessionResetIndex(messages), -1, "local_command compact is not a reset marker");
  eq(trimAfterSessionReset(messages).map((m) => m.id), ["u1", "lc1", "a1"], "non-reset local_command does not trim");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
