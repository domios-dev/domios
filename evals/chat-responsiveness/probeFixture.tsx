import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import { performance } from "node:perf_hooks";
import assert from "node:assert/strict";
import { createTranscriptState, reduceTranscript, transcriptRows, projectTranscript, type NormalizedChatMessage, type TranscriptBatch } from "@codeterm/chat-core";
import { commitTranscriptBatch, reconcileTranscriptWindow, expandTranscriptWindowGap, readTranscriptWindow, useTranscriptWindow, clearTranscriptWindows } from "../../src/components/Terminal/transcriptStore";
import { useChatWindowView } from "../../src/components/Terminal/useChatWindowView";
import { ChatWindowRow } from "../../src/components/Terminal/ChatWindowRow";
import { MessageBubble } from "../../packages/chat-ui/src/MessageBubble";
import { conversationEntries } from "../../src/components/Terminal/chatPagingModel";
import { runtimeReduce } from "../../plugins/provider-codex/src/runtimeReduce";
import type { RuntimeContext, JsonValue } from "@codeterm/plugin-sdk";

const authority = { sourceSessionId: "opaque-session", sessionBindingNonce: "opaque-nonce" };
const key = "opaque-tab:opaque-session";
const context: RuntimeContext = { epoch: 1, tabId: "opaque-tab", bindingNonce: "opaque-nonce", cwd: "/opaque", hostVersion: "1.18.0", threadId: "opaque-session", settings: {}, providerSettings: {}, claimedSends: [], activeTurns: [], threadParents: [] };
const content = "x".repeat(256);
const rows = (count: number, prefix: string): NormalizedChatMessage[] => Array.from({ length: count }, (_, i) => ({ id: `${prefix}-${i}`, type: "assistant", content, sourceSessionId: authority.sourceSessionId }));
const stats = (samples: number[]) => { const sorted = [...samples].sort((a,b) => a-b); return { count: sorted.length, p50: sorted[Math.floor(sorted.length * .5)], p95: sorted[Math.min(sorted.length-1, Math.floor(sorted.length * .95))], max: sorted.at(-1) }; };

export async function runProbe(flush: () => void) {
  for (const expanded of [0, 1000, 10000]) {
    const host = document.createElement("main"); document.body.append(host);
    const gap: NormalizedChatMessage = { id: "gap", type: "system", content: "", timelineMarker: { kind: "history_gap", source: "provider_session", session_id: "opaque-archive", before_offset: 1 } };
    const initial = [gap, ...rows(439, "retained")];
    reconcileTranscriptWindow(key, { kind: "snapshot", authority, range: null, rows: initial });
    if (expanded) expandTranscriptWindowGap(key, "gap", [...rows(expanded, "expanded"), gap]);
    let view!: ReturnType<typeof useChatWindowView>;
    const stage: Record<string, number[]> = { reducer: [], projection: [], entries: [], eventToMockDOM: [] };
    const visits: Record<string, number[]> = { reducer: [], projection: [], entries: [] };
    let activeStage = "";
    let rowVisits = 0;
    const wrap = (row: NormalizedChatMessage) => new Proxy(row, { get(target, property, receiver) { if (activeStage && property === "id") rowVisits++; return Reflect.get(target,property,receiver); } });
    const measure = <T,>(name: string, run: () => T): T => { rowVisits = 0; activeStage = name; const start = performance.now(); const result = run(); stage[name].push(performance.now()-start); visits[name].push(rowVisits); activeStage = ""; return result; };
    const root = createRoot(host);
    function Harness() {
      const stored = useTranscriptWindow(key);
      const followTailRef = useRef(true);
      view = useChatWindowView({ tabId: "opaque-tab", agentSessionId: "opaque-session", windowKey: key, transcript: stored.transcript, anchorKey: stored.anchorKey, followTailRef, loadOlderPage: async () => null, loadHistoryPage: () => new Promise(() => {}) });
      return createElement("section", null, createElement("textarea", { "aria-label": "Probe input" }), view.enrichedMessages.map(entry => createElement(ChatWindowRow, { key: entry.msg.id, message: entry.msg, children: createElement(MessageBubble, { message: entry.msg, item: entry.item.kind === "tool" ? entry.item : undefined }) })));
    }
    await act(async () => root.render(createElement(Harness)));
    let providerState: JsonValue = null;
    const observed: { method: string; rows: number; status?: string; summary?: string }[] = [];
    for (let index = 0; index < 20; index++) {
      const method = index === 0 ? "turn/started" : index === 1 ? "item/started" : index === 19 ? "item/completed" : "item/commandExecution/outputDelta";
      const params = { threadId: "opaque-session", turnId: "opaque-turn", turn: { id: "opaque-turn" }, itemId: "opaque-tool", delta: `opaque-progress-${index}\n`, item: { id: "opaque-tool", type: "commandExecution", command: "opaque-current-operation", status: index === 19 ? "completed" : "inProgress", aggregatedOutput: index === 19 ? "opaque-progress-18" : "" } };
      const reduced = runtimeReduce(context, providerState, [{ kind: "notification", method, params }]); providerState = reduced.state;
      const incoming = reduced.events.flatMap(event => event.kind === "chatItem" ? [event.item as NormalizedChatMessage] : []);
      observed.push({ method, rows: incoming.length, status: incoming[0]?.toolStatus, summary: incoming[0]?.toolSummary });
      const batch: TranscriptBatch = { kind: "delta", authority, range: null, rows: incoming };
      const stored = readTranscriptWindow(key).transcript;
      const instrumented = { ...stored, segments: stored.segments.map(segment => ({ ...segment, rows: segment.rows.map(wrap) })) };
      const next = measure("reducer", () => reduceTranscript(instrumented,batch));
      const projected = measure("projection", () => projectTranscript(next.state));
      measure("entries", () => conversationEntries(projected.rows));
      const start = performance.now();
      await act(async () => { commitTranscriptBatch(key,batch); flush(); });
      stage.eventToMockDOM.push(performance.now()-start);
      if (index > 1) assert.ok(host.textContent?.includes("opaque-current-operation"), "current command reaches rendered bubble before completion");
      const input = host.querySelector("textarea")!; input.value = `opaque-input-${index}`; input.setSelectionRange(3,3); input.dispatchEvent(new Event("input", { bubbles: true })); assert.equal(input.selectionStart,3);
    }
    console.log(JSON.stringify({ expanded, retained: 440, historicalContentBytes: (439+expanded)*256, finalRows: transcriptRows(readTranscriptWindow(key).transcript).length, stages: Object.fromEntries(Object.entries(stage).map(([name,samples]) => [name,stats(samples)])), idReads: Object.fromEntries(Object.entries(visits).map(([name,samples]) => [name,stats(samples)])), observed, visibleOperation: host.textContent?.includes("opaque-current-operation"), newestToolResult: view.allMessages.at(-1)?.toolResult }));
    await act(async () => root.unmount()); host.remove(); clearTranscriptWindows();
  }
}
