import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, test } from "vitest";
import { transcriptRows } from "@codeterm/chat-core";
import { awaitsTranscriptEcho, type DeliverySend } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";
import { HoldWhileHidden } from "./HoldWhileHidden";
import {
  clearTranscriptWindows,
  commitTranscriptBatch,
  markTranscriptSnapshotReady,
  readTranscriptWindow,
  useTranscriptWindow,
} from "./transcriptStore";

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });

const KEY = "tab-held:session";
let renders = 0;

function Transcript({ live }: { live: boolean }) {
  renders++;
  const rows = transcriptRows(useTranscriptWindow(KEY, live).transcript);
  return <output aria-label="Rows">{rows.length}:{rows[rows.length - 1]?.id ?? ""}</output>;
}

async function streamDelta(id: string, seq: number): Promise<void> {
  await act(async () => {
    commitTranscriptBatch(KEY, {
      kind: "delta",
      authority: { sourceSessionId: "session", sessionBindingNonce: "nonce" },
      range: null,
      rows: [{ id, type: "assistant", content: `fragment ${seq}`, seq }],
    });
    await new Promise((resolve) => requestAnimationFrame(resolve));
  });
}

afterEach(() => {
  clearTranscriptWindows();
  document.body.replaceChildren();
});

test("a held transcript view skips streamed deltas and catches up in one render when live", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const shown = () => host.querySelector("output")?.textContent;
  await act(async () => root.render(<Transcript live />));
  await streamDelta("a", 1);
  assert.equal(shown(), "1:a");

  await act(async () => root.render(<Transcript live={false} />));
  const heldRenders = renders;
  await streamDelta("b", 2);
  await streamDelta("c", 3);
  assert.equal(renders, heldRenders);
  assert.equal(shown(), "1:a");
  assert.equal(transcriptRows(readTranscriptWindow(KEY).transcript).length, 3);

  await act(async () => root.render(<Transcript live />));
  assert.equal(renders, heldRenders + 1);
  assert.equal(shown(), "3:c");
  await streamDelta("d", 4);
  assert.equal(shown(), "4:d");
  await act(async () => root.unmount());
});

test("a held view keeps its window from eviction", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await streamDelta("a", 1);
  await act(async () => root.render(<Transcript live={false} />));
  for (let index = 0; index < 12; index++) markTranscriptSnapshotReady(`other-${index}`, "nonce");
  assert.equal(transcriptRows(readTranscriptWindow(KEY).transcript).length, 1);
  await act(async () => root.unmount());
});

let rowRenders = 0;
function Row({ value }: { value: number }) {
  rowRenders++;
  return <output aria-label="Row">{value}</output>;
}

test("hidden rows keep their last render until the chat is visible again", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const shown = () => host.querySelector("output")?.textContent;
  await act(async () => root.render(<HoldWhileHidden visible><Row value={1} /></HoldWhileHidden>));
  await act(async () => root.render(<HoldWhileHidden visible={false}><Row value={2} /></HoldWhileHidden>));
  await act(async () => root.render(<HoldWhileHidden visible={false}><Row value={3} /></HoldWhileHidden>));
  assert.equal(rowRenders, 1);
  assert.equal(shown(), "1");
  await act(async () => root.render(<HoldWhileHidden visible><Row value={4} /></HoldWhileHidden>));
  assert.equal(rowRenders, 2);
  assert.equal(shown(), "4");
  await act(async () => root.unmount());
});

const send = (status?: ChatSendStatus["state"], extra: Partial<DeliverySend> = {}): DeliverySend => ({
  id: "bubble", correlationId: "token", content: "opaque", sentAtMs: 1000, ...extra,
  ...(status ? { status: { tab_id: "tab", send_id: "transaction", state: status, sequence: 1, ledger_id: "ledger",
    retained_from: 1, error: null, correlationId: "token" } } : {}),
});

test("only unsettled, undismissed sends keep a hidden transcript live", () => {
  assert.equal(awaitsTranscriptEcho([]), false);
  assert.equal(awaitsTranscriptEcho([send()]), true);
  assert.equal(awaitsTranscriptEcho([send("held")]), true);
  assert.equal(awaitsTranscriptEcho([send("submit_unconfirmed")]), true);
  assert.equal(awaitsTranscriptEcho([send("delivered")]), false);
  assert.equal(awaitsTranscriptEcho([send("failed")]), false);
  assert.equal(awaitsTranscriptEcho([send(undefined, { echo: { content: "opaque", timestampMs: 1 } })]), false);
  assert.equal(awaitsTranscriptEcho([send(undefined, { dismissed: true })]), false);
});
