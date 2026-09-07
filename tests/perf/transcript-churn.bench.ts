import {
  mergeById,
  attachToolResults,
  trimAfterSessionReset,
  matchedOptimisticEchoes,
} from "../../packages/chat-core/src/index";
import type { NormalizedChatMessage } from "../../packages/chat-core/src/types";

const BASE_TS = 1_700_000_000_000;

function makeMessage(i: number): NormalizedChatMessage {
  const kind = i % 4;
  if (kind === 0) return { id: `u-${i}`, type: "user", content: `user turn ${i}`, timestamp: String(BASE_TS + i * 1000), seq: i };
  if (kind === 1) return { id: `a-${i}`, type: "assistant", content: `assistant reply ${i} `.repeat(20), timestamp: String(BASE_TS + i * 1000), seq: i };
  if (kind === 2) return { id: `tc-${i}`, type: "tool_call", content: "", toolId: `t-${i}`, toolName: "bash", toolArgs: `cmd ${i}`, timestamp: String(BASE_TS + i * 1000), seq: i };
  return { id: `tr-${i}`, type: "tool_result", content: `result output ${i} `.repeat(40), toolId: `t-${i - 1}`, timestamp: String(BASE_TS + i * 1000), seq: i };
}

function buildTranscript(n: number): NormalizedChatMessage[] {
  const m: NormalizedChatMessage[] = [];
  for (let i = 0; i < n; i++) m.push(makeMessage(i));
  return m;
}

// One streamed chunk: the tail assistant message is re-emitted with a growing
// delta — exactly what codex does per streaming token batch.
function simulateChunk(prev: NormalizedChatMessage[], deltaLen: number): NormalizedChatMessage[] {
  const tail = prev[prev.length - 1];
  const updated = { ...tail, content: (tail.content ?? "") + "x".repeat(deltaLen) };
  const merged = mergeById(prev, [updated]);
  const reset = trimAfterSessionReset(merged);
  const grouped = attachToolResults(reset);
  matchedOptimisticEchoes([], grouped.filter((m) => m.type === "user"));
  return merged;
}

function pct(s: number[], p: number): number {
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
}

const CHUNKS = 60;
console.log("Transcript churn latency per streamed chunk (ms) — frame budget 16.7ms\n");
console.log("   N      p50      p95      max    frames>16.7ms");
for (const n of [200, 500, 1000, 2000, 5000, 10000]) {
  let t = buildTranscript(n);
  const s: number[] = [];
  for (let i = 0; i < 5; i++) t = simulateChunk(t, 8);
  let over = 0;
  for (let i = 0; i < CHUNKS; i++) {
    const t0 = performance.now();
    t = simulateChunk(t, 8);
    const dt = performance.now() - t0;
    s.push(dt);
    if (dt > 16.7) over++;
  }
  s.sort((a, b) => a - b);
  console.log(`${String(n).padStart(5)}  ${pct(s, 50).toFixed(3).padStart(7)}  ${pct(s, 95).toFixed(3).padStart(7)}  ${s[s.length - 1].toFixed(3).padStart(7)}   ${over}/${CHUNKS}`);
}
