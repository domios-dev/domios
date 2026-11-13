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

test("bubble states stay dot-only and recovery appears only on activation", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  let retried = 0, removed = 0, edited = 0;
  for (const state of ["sending", "written", "queued", "uncertain", "failed", "delivered"] as const) {
    await act(async () => root.render(<ChatDeliveryState state={state} queuedBehindTurn
      onRetry={() => retried++} onRemove={() => removed++} onEdit={() => edited++} />));
    const status = host.querySelector('[role="status"]');
    if (state === "delivered") { assert.ok(!status); continue; }
    assert.ok(status?.getAttribute("aria-label"));
    assert.equal(status?.getAttribute("title"), status?.getAttribute("aria-label"));
    assert.equal(status?.textContent, "");
    assert.ok(!document.querySelector('[role="dialog"]'));
    const trigger = status?.querySelector("button");
    if (state !== "failed" && state !== "uncertain") { assert.ok(!trigger); continue; }
    assert.equal(trigger?.getAttribute("aria-label"), status?.getAttribute("aria-label"));
    await act(async () => trigger!.click());
    const buttons = [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')];
    assert.deepEqual(buttons.map(button => button.textContent), state === "failed" ? ["Retry", "Remove"] : ["Edit and resend", "Remove"]);
    for (let i = 0; i < buttons.length; i++) {
      if (i > 0) await act(async () => trigger!.click());
      await act(async () => document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')[i].click());
    }
  }
  assert.equal(retried, 1); assert.equal(edited, 1); assert.equal(removed, 2);
  await act(async () => root.unmount());
});

test("failure and refusal reasons remain accessible without a bubble label", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  for (const error of ["runtime_rejected_before_write: missing image", "runtime_preflight_before_write: binding changed", "upload rejected", undefined]) {
    await act(async () => root.render(<ChatDeliveryState state="failed" error={error} onRetry={() => {}} />));
    const status = host.querySelector('[role="status"]');
    assert.ok(status?.getAttribute("aria-label")?.includes(error?.split(": ").slice(-1)[0] ?? "failure reason is unavailable"));
    assert.equal(status?.textContent, "");
    assert.equal(status?.getAttribute("title"), status?.getAttribute("aria-label"));
  }
  for (const state of ["sending", "written", "queued", "uncertain", "failed"] as const) {
    await act(async () => root.render(<ChatDeliveryState inline state={state} />));
    const status = host.querySelector('[role="status"]');
    assert.equal(status?.textContent, status?.getAttribute("aria-label"));
  }
  await act(async () => root.unmount());
});

const message: NormalizedChatMessage = { id: "cli-send", type: "user", content: "opaque routed body" };

test("dot recovery focuses its actions, dismisses on Escape and disappears on delivery", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<ChatDeliveryState state="uncertain" onEdit={() => {}} onRemove={() => {}} />));
  const trigger = host.querySelector("button")!;
  assert.equal(trigger.tagName, "BUTTON");
  assert.equal(trigger.getAttribute("aria-haspopup"), "dialog");
  await act(async () => trigger.click());
  assert.equal(document.activeElement?.textContent, "Edit and resend");
  await act(async () => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true })));
  assert.ok(!document.querySelector('[role="dialog"]'));
  assert.equal(document.activeElement, trigger);
  await act(async () => trigger.click());
  await act(async () => root.render(<ChatDeliveryState state="delivered" />));
  assert.ok(!document.querySelector('[role="dialog"]'));
  await act(async () => root.unmount());
});

test("inline recovery keeps visible labels and actions", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  for (const state of ["failed", "uncertain"] as const) {
    await act(async () => root.render(<ChatDeliveryState inline state={state} onRetry={() => {}} onEdit={() => {}} onRemove={() => {}} />));
    const status = host.querySelector('[role="status"]')!;
    assert.ok(status.textContent?.startsWith(status.getAttribute("aria-label")!));
    assert.deepEqual([...host.querySelectorAll("button")].map(button => button.textContent), state === "failed" ? ["Retry", "Remove"] : ["Edit and resend", "Remove"]);
    assert.ok(!document.querySelector('[role="dialog"]'));
  }
  await act(async () => root.unmount());
});

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
