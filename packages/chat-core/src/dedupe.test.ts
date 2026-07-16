import { projectTranscriptMessages } from "./dedupe";
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

const sessionId = "c73becde";
const startupText = "You are Theo. You are a frontend worker fixing a transcript projection/dedup defect.";

function parsed(id: string, seq: number, content = startupText): NormalizedChatMessage {
  return { id, seq, type: "user", content, timestamp: "2026-07-16T10:00:00Z" };
}

function optimistic(id: string, seq: number, content = startupText): NormalizedChatMessage {
  return {
    id,
    seq,
    type: "user",
    content,
    timestamp: "1784196000000",
    transcriptIdentity: `session:${sessionId}:seq:${seq}`,
  };
}

// RED before identity projection: parsed seq-1 plus its local launch echo
// rendered as two identical bubbles on c73becde.
{
  const projection = projectTranscriptMessages(
    [parsed("codex-persisted-seq-1", 1)],
    [optimistic("opt-launch", 1)],
    sessionId,
  );
  eq(projection.map((message) => message.id), ["codex-persisted-seq-1"], "c73becde startup projects one bubble");
}

// Identical content is not identity: separate parser sequences remain visible.
{
  const projection = projectTranscriptMessages(
    [parsed("persisted-repeat-1", 1, "continue"), parsed("persisted-repeat-2", 2, "continue")],
    [],
    sessionId,
  );
  eq(projection.map((message) => message.id), ["persisted-repeat-1", "persisted-repeat-2"], "distinct stable ids preserve true repeats");
}

// Re-running projection after reload has no stateful/content-based second pass.
{
  const confirmed = [parsed("codex-persisted-seq-1", 1)];
  const overlay = [optimistic("opt-launch", 1)];
  const first = projectTranscriptMessages(confirmed, overlay, sessionId);
  const reloaded = projectTranscriptMessages(confirmed, overlay, sessionId);
  eq(reloaded.map((message) => message.id), first.map((message) => message.id), "reload projection is idempotent");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
