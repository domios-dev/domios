import { act, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { createTranscriptState, reduceTranscript, type NormalizedChatMessage } from "@codeterm/chat-core";
import { clearTranscriptWindows, useTranscriptWindow } from "./transcriptStore";
import { useChatWindowView } from "./useChatWindowView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const authority = { sourceSessionId: "paging", sessionBindingNonce: "nonce" };
const rows = (start: number, end: number): NormalizedChatMessage[] => Array.from({ length: end - start }, (_, index) => ({
  id: `m-${start + index}`, type: "assistant", content: `opaque-${start + index}`,
}));
let root: Root | undefined;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  clearTranscriptWindows();
  document.body.replaceChildren();
});

async function mount(start: number, end: number, loadedRows = rows(start, end),
  loadHistoryPage?: (message: NormalizedChatMessage) => Promise<NormalizedChatMessage[] | null>, sparse = false) {
  let view!: ReturnType<typeof useChatWindowView>;
  let requests = 0;
  const host = document.createElement("main");
  document.body.append(host);
  function Harness() {
    const [transcript, setTranscript] = useState(() => reduceTranscript(createTranscriptState(), {
      kind: "snapshot", authority, range: { start, end, unit: "items" }, rows: loadedRows,
    }).state);
    const followTailRef = useRef(true);
    const { anchorKey } = useTranscriptWindow("paging:session");
    view = useChatWindowView({ tabId: "paging", windowKey: "paging:session", transcript, anchorKey, followTailRef,
      loadHistoryPage,
      loadOlderPage: async cursor => {
        requests++;
        if (sparse && requests <= 2) {
          setTranscript(current => reduceTranscript(current, { kind: "backfill", authority,
            range: { start: cursor - 1, end: cursor, unit: "items" }, rows: [] }).state);
          return { cursor: cursor - 1, consumedBytes: 1024 * 1024, rows: 0 };
        }
        const from = Math.max(0, cursor - 150);
        const messages = rows(from, cursor);
        setTranscript(current => reduceTranscript(current, { kind: "backfill", authority,
          range: { start: from, end: cursor, unit: "items" }, rows: messages }).state);
        return { cursor: from, consumedBytes: 0, rows: messages.length, messages: [...messages, ...loadedRows] };
      },
    });
    return <><button onClick={() => void view.loadOlder()}>{view.olderMessageLabel}</button>
      <output>{view.enrichedMessages.map(entry => entry.msg.id).join(",")}</output></>;
  }
  root = createRoot(host);
  await act(async () => root!.render(<Harness />));
  return { host, view: () => view, requests: () => requests };
}

it("opens at 100, reveals labelled 100s, clamps the final step and has no hidden remainder", async () => {
  const mounted = await mount(0, 303);
  expect(mounted.view().enrichedMessages).toHaveLength(100);
  for (const count of [200, 300, 300]) {
    const previous = mounted.view().enrichedMessages.map(entry => entry.msg.id);
    const label = mounted.host.querySelector("button")!.textContent;
    expect(label).toContain(String(Math.min(100, 303 - previous.length)));
    await act(async () => mounted.host.querySelector("button")!.click());
    expect(mounted.view().enrichedMessages).toHaveLength(count);
    expect(mounted.view().enrichedMessages.map(entry => entry.msg.id).filter(id => previous.includes(id)).slice(0, 1)).toEqual(previous.slice(0, 1));
  }
  expect(mounted.view().hiddenMessageCount).toBe(0);
  expect(mounted.view().canBackfillOlder).toBe(false);
  expect(mounted.requests()).toBe(0);
});

it("fills an initially short raw snapshot in visible units, then reveals only one page", async () => {
  const mounted = await mount(300, 305);
  expect(mounted.requests()).toBe(1);
  expect(mounted.view().enrichedMessages).toHaveLength(100);
  expect(mounted.view().enrichedMessages[0].msg.id).toBe("m-205");
  await act(async () => mounted.host.querySelector("button")!.click());
  expect(mounted.requests()).toBe(2);
  expect(mounted.view().enrichedMessages).toHaveLength(200);
  expect(mounted.view().enrichedMessages[0].msg.id).toBe("m-105");
});

it("archives use the same 100-message step, reuse fetched rows and remove the exhausted gap", async () => {
  const gap: NormalizedChatMessage = { id: "archive-gap", type: "system", content: "opaque",
    timelineMarker: { kind: "history_gap", source: "provider_session", session_id: "archive", before_offset: 100 } };
  let loads = 0;
  const mounted = await mount(0, 2, [...rows(1000, 1001), gap, ...rows(1001, 1002)], async () => { loads++; return rows(0, 245); });
  for (const count of [103, 203, 247]) {
    await act(async () => mounted.view().loadHistoryGap(gap));
    expect(mounted.view().enrichedMessages).toHaveLength(count);
  }
  expect(loads).toBe(1);
  expect(mounted.view().enrichedMessages.some(entry => entry.msg.id === gap.id)).toBe(false);
});


it("a leading archive gap uses only the top paging control and disappears when exhausted", async () => {
  const gap: NormalizedChatMessage = { id: "top-gap", type: "system", content: "opaque-gap",
    timelineMarker: { kind: "history_gap", source: "provider_session", session_id: "archive", before_offset: 100 } };
  let loads = 0;
  const mounted = await mount(0, 101, [gap, ...rows(300, 400)], async () => { loads++; return rows(0, 125); });
  expect(mounted.view().enrichedMessages).toHaveLength(100);
  expect(mounted.view().enrichedMessages.some(entry => entry.msg.id === gap.id)).toBe(false);
  await act(async () => mounted.host.querySelector("button")!.click());
  expect(loads).toBe(1);
  expect(mounted.view().enrichedMessages).toHaveLength(200);
  await act(async () => mounted.host.querySelector("button")!.click());
  expect(loads).toBe(1);
  expect(mounted.view().enrichedMessages).toHaveLength(225);
  expect(mounted.view().canBackfillOlder).toBe(false);
  expect(mounted.view().hiddenMessageCount).toBe(0);
});


it("a scan-limited initial window reveals exactly one batch without filling unused capacity", async () => {
  const mounted = await mount(300, 367, rows(300, 367), undefined, true);
  expect(mounted.view().enrichedMessages).toHaveLength(67);
  expect(mounted.view().backfillNotice).toBe("limit");
  const previous = mounted.view().enrichedMessages.map(entry => entry.msg.id);
  await act(async () => mounted.host.querySelector("button")!.click());
  expect(mounted.view().enrichedMessages).toHaveLength(167);
  expect(mounted.view().enrichedMessages.map(entry => entry.msg.id).slice(-67)).toEqual(previous);
});
