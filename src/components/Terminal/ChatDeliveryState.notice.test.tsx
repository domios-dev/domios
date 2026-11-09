// WHAT: a held CLI send renders as a System · CLI notice card.
// ORACLE: its queued state stays visible in the collapsed header; delivery removes it from the same card.
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, test } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "@codeterm/chat-ui";
import { ChatDeliveryState } from "./ChatDeliveryState";
import type { DeliveryState } from "./chatDeliveryModel";

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });

afterEach(() => document.body.replaceChildren());

test("every pending state exposes its reason and available controls without interaction", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let retried = 0;
  let removed = 0;
  let edited = 0;
  for (const state of ["sending", "written", "queued", "uncertain", "failed", "delivered"] as const) {
    await act(async () => root.render(<ChatDeliveryState state={state} onRetry={() => retried++}
      onRemove={() => removed++} onEdit={() => edited++} />));
    const status = host.querySelector('[role="status"]');
    if (state === "delivered") { assert.equal(status, null); continue; }
    assert.ok(status?.textContent);
    const buttons = [...host.querySelectorAll("button")];
    assert.deepEqual(buttons.map(button => button.textContent), state === "failed" ? ["Retry", "Remove"]
      : state === "uncertain" ? ["Edit and resend", "Remove"] : ["Remove"]);
    for (const button of buttons) await act(async () => button.click());
  }
  assert.equal(retried, 1);
  assert.equal(edited, 1);
  assert.equal(removed, 5);
  await act(async () => root.unmount());
});

const message: NormalizedChatMessage = { id: "cli-send", type: "user", content: "opaque routed body" };

function Notice({ state }: { state: DeliveryState }) {
  return <MessageBubble message={message} origin={{ kind: "system", context: "CLI" }}
    noticeStatus={state === "delivered" ? undefined : <ChatDeliveryState inline state={state} queuedBehindTurn />} />;
}

test("a held CLI notice shows its queued state while collapsed and drops it once delivered", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Notice state="queued" />));
  const card = host.querySelector<HTMLElement>('[data-testid="chat-system-event"]');
  assert.ok(card);
  assert.equal(card.querySelector("button[aria-expanded]")?.getAttribute("aria-expanded"), "false");
  const status = card.querySelector<HTMLElement>('.ct-delivery[role="status"]');
  assert.equal(status?.dataset.state, "queued");
  assert.ok(status?.querySelector(".ct-delivery__label:not(.sr-only)")?.textContent);

  await act(async () => root.render(<Notice state="delivered" />));
  assert.equal(host.querySelectorAll('[data-testid="chat-system-event"]').length, 1);
  assert.equal(host.querySelector('.ct-delivery[role="status"]'), null);
  await act(async () => root.unmount());
});
