import { readFileSync } from "node:fs";
import path from "node:path";
import { act, createElement, useLayoutEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { addChatSend, __setChatSendRepositoryForTest, ensureChatSendTabHydrated, forgetChatSendsInMemory } from "./chatSendStore";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";
import { clearTranscriptWindows } from "./transcriptStore";
import type { ChatStartResult, ChatBackfillResult } from "./chatSessionAuthority";
import { useChatSession } from "./useChatSession";
import { stressStart, stressBackfill, stressPages, stressReplyIds } from "../../../tests/fixtures/chat/imageStress";

interface Capture {
  name: string;
  reply_id: string;
  body_start: number;
  start: ChatStartResult & { messages: NormalizedChatMessage[] };
  pages: ChatBackfillResult[];
  reply_order?: string[];
  reachesStart?: boolean;
}
// Produced by the native air_chat_start selector; omitted image bodies retain their byte offsets.
const captures = JSON.parse(readFileSync(path.resolve(import.meta.dirname,
  "../../../tests/fixtures/chat/air-start-wire.json"), "utf8")) as Capture[];
captures.push({ name: "stress-cached-order", reply_id: stressReplyIds[0], body_start: 815808,
  start: stressStart, pages: [stressBackfill, ...stressPages],
  reply_order: stressStart.messages.filter(row => row.type === "assistant").map(row => row.id), reachesStart: true });
let mounted: Root | null = null;
afterEach(async () => {
  await act(async () => mounted?.unmount());
  mounted = null;
  clearTranscriptWindows();
  forgetChatSendsInMemory();
  __setChatSendRepositoryForTest(null);
  __setInvokeForTest(null);
  __setListenForTest(null);
  document.body.replaceChildren();
});

test.each(captures)("air $name: cold start and remount automatically find the recent reply", async capture => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  __setChatSendRepositoryForTest(new MemoryChatSendRepository());
  __setListenForTest(async () => () => {});
  const tabId = `oversized-${capture.name}`;
  const requests: Array<{ before: number; maxBytes: number }> = [];
  let starts = 0;
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    if (command === "chat_start") { starts++; return { ...capture.start, subscription_id: args?.subscriptionId } as T; }
    if (command === "chat_resume") return { kind: "resumed", deltas: [], caught_up: true } as T;
    if (command === "chat_backfill") {
      const before = Number(args?.beforeOffset);
      const page = capture.pages.find(page => page.end_offset === before);
      expect(page, `captured complete page before ${before}`).toBeDefined();
      requests.push({ before, maxBytes: Number(args?.maxBytes) });
      return page as T;
    }
    if (command === "chat_send_status" || command === "chat_send_dismissals") return [] as T;
    return {} as T;
  });
  await ensureChatSendTabHydrated(tabId);
  addChatSend(tabId, { id: "persisted-owner", content: "retained local owner send", sentAtMs: 1_793_950_000_000,
    correlationId: "persisted-owner", echoBoundary: { sourceSessionId: capture.start.source_session_id!,
      sessionBindingNonce: capture.start.session_binding_nonce, lastRowId: "outside-window", endOffset: capture.body_start } });
  for (const phase of ["cold", "remount"]) {
    requests.length = 0;
    let settled!: () => void;
    const filled = new Promise<void>(resolve => { settled = resolve; });
    let view: ReturnType<typeof useChatSession> | null = null;
    function Harness({ active = true }: { active?: boolean }) {
      const current = useChatSession({ tabId, agentSessionId: capture.start.source_session_id,
        sessionBindingNonce: capture.start.session_binding_nonce, active });
      view = current;
      useLayoutEffect(() => {
        if (current.snapshotReady && !current.loading && !current.backfillLoading && requests.length > 0) settled();
      });
      return createElement("div", null, current.enrichedMessages.map(({ msg }) =>
        createElement("article", { key: msg.id, "data-row-id": msg.sourceMessageId ?? msg.id }, msg.content)));
    }
    const host = document.createElement("div");
    document.body.append(host);
    mounted = createRoot(host);
    await act(async () => { mounted!.render(createElement(Harness)); });
    await act(async () => { await filled; });
    const current = view as unknown as ReturnType<typeof useChatSession>;
    expect(current.allMessages.filter(row => row.id === capture.reply_id), phase).toHaveLength(1);
    expect(host.querySelectorAll(`[data-row-id="${capture.reply_id}"]`), phase).toHaveLength(1);
    expect(requests.length).toBeLessThanOrEqual(8);
    expect(requests.every(request => request.maxBytes > 0 && request.maxBytes <= 1024 * 1024)).toBe(true);
    for (let index = 1; index < requests.length; index++) expect(requests[index].before).toBeLessThan(requests[index - 1].before);
    expect(current.canBackfillOlder).toBe(!capture.reachesStart);
    if (capture.reply_order) {
      const assertOrder = () => {
        const projected = view as unknown as ReturnType<typeof useChatSession>;
        expect(projected.allMessages.filter(row => capture.reply_order!.includes(row.id)).map(row => row.id)).toEqual(capture.reply_order);
        expect([...host.querySelectorAll("article")].map(node => node.getAttribute("data-row-id"))
          .filter(id => capture.reply_order!.includes(id!))).toEqual(capture.reply_order);
      };
      assertOrder();
      const previousStarts = starts;
      await act(async () => mounted!.render(createElement(Harness, { active: false })));
      await act(async () => mounted!.render(createElement(Harness, { active: true })));
      expect(starts).toBe(previousStarts + 1);
      assertOrder();
    }
    await act(async () => { mounted!.unmount(); });
    mounted = null;
    clearTranscriptWindows();
    forgetChatSendsInMemory();
  }
});
