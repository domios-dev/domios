// Caller: scripts bench-chat-stream.mjs
import React, { useDeferredValue, useMemo } from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { MessageBubble, MessagePartsContext } from "@codeterm/chat-ui";
import { transcriptRows, type NormalizedChatMessage } from "@codeterm/chat-core";
import { commitTranscriptBatch, readTranscriptWindow, useTranscriptWindow } from "../src/components/Terminal/transcriptStore";
import { indexMessageParts } from "../src/components/Terminal/messagePartSelection";

const key = "benchmark:stream";
const authority = { sourceSessionId: "stream", sessionBindingNonce: "nonce" };
let renders = 0;

function Chat({ streaming = true }: { streaming?: boolean }) {
  const renderingStream = useDeferredValue(streaming);
  const { transcript } = useTranscriptWindow(key);
  const rows = transcriptRows(transcript);
  const index = useMemo(() => indexMessageParts(rows.map((msg) => ({
    id: msg.id, role: "assistant" as const, source: msg.content, author: "Agent",
  })), renderingStream ? rows.at(-1)?.id : null), [transcript, renderingStream]);
  renders++;
  return <>{rows.map((message) => <MessagePartsContext.Provider key={message.id} value={{
    id: message.id, source: message.content, selecting: false,
    selected: () => false, joinNext: () => false, cursor: () => false, preview: null,
  }}><article aria-label={`Assistant ${index.byMessage.get(message.id)?.length}`}>
    <MessageBubble message={message} streaming={streaming} />
  </article></MessagePartsContext.Provider>)}</>;
}

const host = document.getElementById("root")!;
const root = createRoot(host);
flushSync(() => root.render(<Chat />));

const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const task = () => new Promise<void>((resolve) => {
  const channel = new MessageChannel();
  channel.port1.onmessage = () => { channel.port1.close(); channel.port2.close(); resolve(); };
  channel.port2.postMessage(null);
});

async function run({ chunkSize = 64, cadence = "frame" }: { chunkSize?: number; cadence?: "frame" | "task" } = {}) {
  const source = Array.from({ length: 600 }, (_, i) => `${i + 1}. **Streaming line ${i + 1}** with \`code\` and [a link](https://example.com).\n`).join("");
  const longTasks: number[] = [];
  const observer = new PerformanceObserver((list) => {
    longTasks.push(...list.getEntries().map((entry) => entry.duration));
  });
  observer.observe({ type: "longtask", buffered: false });
  const started = performance.now();
  for (let end = chunkSize; end < source.length + chunkSize; end += chunkSize) {
    const message: NormalizedChatMessage = { id: "answer", type: "assistant", content: source.slice(0, end), seq: 0 };
    commitTranscriptBatch(key, { kind: "delta", authority, range: null, rows: [message] });
    if (cadence === "frame") await frame();
    else await task();
  }
  await frame();
  await frame();
  await task();
  if (transcriptRows(readTranscriptWindow(key).transcript)[0]?.content !== source) throw new Error("Streamed text was lost or reordered");
  if (host.textContent !== source) throw new Error("Visible streaming text was lost or reordered");
  const maxStreamingLongTaskMs = Math.max(0, ...longTasks);
  const elapsedMs = performance.now() - started;
  const settleStart = performance.now();
  const settled = new Promise<void>((resolve) => {
    const mutations = new MutationObserver(() => {
      if (host.querySelectorAll("strong").length === 600) { mutations.disconnect(); resolve(); }
    });
    mutations.observe(host, { childList: true, subtree: true });
  });
  flushSync(() => root.render(<Chat streaming={false} />));
  await settled;
  await frame();
  await task();
  observer.disconnect();
  if (host.querySelectorAll("strong").length !== 600) throw new Error("Settled markdown missing");
  return { chars: source.length, renders, elapsedMs, settleMs: performance.now() - settleStart, maxStreamingLongTaskMs, longTasks, maxLongTaskMs: Math.max(0, ...longTasks) };
}

Object.assign(window, { runChatStreamBenchmark: run });
