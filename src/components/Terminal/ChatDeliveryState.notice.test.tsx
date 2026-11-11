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

test("failed input exposes marked, unmarked and unknown reasons without changing its actions", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  for (const error of ["runtime_rejected_before_write: missing image", "runtime_preflight_before_write: binding changed", "upload rejected", undefined]) {
    await act(async () => root.render(<ChatDeliveryState state="failed" error={error} onRetry={() => {}} />));
    const status = host.querySelector('[role="status"]');
    assert.ok(status?.textContent?.includes(error?.split(": ").slice(-1)[0] ?? "failure reason is unavailable"));
    assert.ok(host.querySelector("button"));
  }
  await act(async () => root.unmount());
});

test("only actionable bubble states show reasons and controls", async () => {
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
    if (state === "delivered") { assert.ok(!status); continue; }
    assert.ok(status?.getAttribute("aria-label"));
    assert.equal(status?.getAttribute("title"), status?.getAttribute("aria-label"));
    if (state === "failed" || state === "uncertain") assert.ok(status?.textContent);
    else assert.equal(status?.textContent, "");
    const buttons = [...host.querySelectorAll("button")];
    assert.deepEqual(buttons.map(button => button.textContent), state === "failed" ? ["Retry", "Remove"]
      : state === "uncertain" ? ["Edit and resend", "Remove"] : []);
    for (const button of buttons) await act(async () => button.click());
  }
  assert.equal(retried, 1);
  assert.equal(edited, 1);
  assert.equal(removed, 2);
  await act(async () => root.unmount());
});

test("held bubbles show a short queued label and inline states keep the full label", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<ChatDeliveryState state="queued" queuedBehindTurn onRemove={() => {}} />));
  const status = host.querySelector('[role="status"]');
  assert.equal(status?.textContent, "Queued");
  assert.equal(status?.getAttribute("aria-label"), "Queued until the current turn ends");
  assert.ok(!host.querySelector("button"));
  for (const state of ["sending", "written", "queued"] as const) {
    await act(async () => root.render(<ChatDeliveryState inline state={state} onRemove={() => {}} />));
    const inlineStatus = host.querySelector('[role="status"]');
    assert.equal(inlineStatus?.textContent, inlineStatus?.getAttribute("aria-label"));
    assert.ok(!host.querySelector("button"));
  }
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
  const status = card.querySelector<HTMLElement>('[role="status"]');
  assert.equal(status?.getAttribute("aria-label"), "Queued until the current turn ends");
  assert.equal(status?.textContent, status?.getAttribute("aria-label"));

  await act(async () => root.render(<Notice state="delivered" />));
  assert.equal(host.querySelectorAll('[data-testid="chat-system-event"]').length, 1);
  assert.ok(!host.querySelector('[role="status"][aria-label="Queued until the current turn ends"]'));
  await act(async () => root.unmount());
});
