import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { MessageBubble } from "./MessageBubble";

const raw = "Opaque quota exhausted for this key. Retry later.\nhttps://opaque.example/quota\nmetric: opaque/requests";

it("renders the handoff as one divider with its payload under Details", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const host = document.createElement("div");
  const root = createRoot(host);
  const content = "opaque handoff payload";
  await act(async () => root.render(<MessageBubble
    showTimestamp={false}
    message={{ id: "handoff:fixture", type: "user", content, inboundKind: "system", fromContext: "provider-handoff",
      fromLabel: "Codex", fromModel: "GPT-6.2-Sol" }}
  />));
  try {
    const dividers = host.querySelectorAll('[role="separator"]');
    expect(Array.from(dividers, (node) => node.getAttribute("aria-label"))).toEqual(["Continued on Codex · GPT-6.2-Sol"]);
    expect(host.querySelector('[aria-label="System"]')).toBeNull();
    const details = host.querySelector("details");
    expect(details?.open).toBe(false);
    expect(details?.querySelector("summary")?.textContent).toBe("Details");
    expect(details?.querySelector("pre")?.textContent).toBe(content);
    expect(host.textContent?.replace(details?.textContent ?? "", "")).toBe("Continued on Codex · GPT-6.2-Sol");
  } finally {
    await act(async () => root.unmount());
  }
});

it.each([
  ["status", undefined],
  ["system", "api_error"],
])("a %s provider error row shows one bounded line and keeps the raw text under Details", async (type, systemSubtype) => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MessageBubble showTimestamp={false} message={{ id: `row:${type}`, type, systemSubtype, content: raw }} />));
    const details = host.querySelector("details");
    expect(details?.querySelector("summary")?.textContent).toBe("Details");
    expect(details?.querySelector("pre")?.textContent).toBe(raw);
    const visible = host.textContent?.replace(details?.textContent ?? "", "");
    expect(visible).toBe("Opaque quota exhausted for this key.");
  } finally {
    await act(async () => root.unmount());
  }
});

it("renders calm dot+word stopReason notices with a ghost Continue action for limit stops only", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const host = document.createElement("div");
  const root = createRoot(host);
  let continued = 0;
  try {
    await act(async () => root.render(<MessageBubble
      showTimestamp={false}
      onContinueTurn={() => { continued += 1; }}
      message={{ id: "turn:1:stop", type: "system", systemSubtype: "turn_stop_reason", toolInput: { stopReason: "max_tokens" }, content: "Stopped at output limit" }}
    />));
    const maxTokens = host.querySelector('[data-testid="chat-turn-stop-reason"]')!;
    expect(maxTokens.getAttribute("role")).toBe("status");
    expect(maxTokens.textContent).toContain("Stopped at output limit");
    expect(maxTokens.textContent).not.toContain("max_tokens");
    const button = maxTokens.querySelector<HTMLButtonElement>("button")!;
    expect(button.textContent).toBe("Continue");
    expect(button.className).toContain("ct-btn-ghost");
    await act(async () => button.click());
    expect(continued).toBe(1);

    await act(async () => root.render(<MessageBubble
      showTimestamp={false}
      onContinueTurn={() => { continued += 1; }}
      message={{ id: "turn:2:stop", type: "system", systemSubtype: "turn_stop_reason", toolInput: { stopReason: "max_turn_requests" }, content: "Stopped at turn limit" }}
    />));
    expect(host.querySelector('[data-testid="chat-turn-stop-reason"]')?.textContent).toContain("Stopped at turn limit");
    expect(host.querySelector('[data-testid="chat-turn-stop-reason"] button')?.textContent).toBe("Continue");

    await act(async () => root.render(<MessageBubble
      showTimestamp={false}
      onContinueTurn={() => { continued += 1; }}
      message={{ id: "turn:3:stop", type: "system", systemSubtype: "turn_stop_reason", toolInput: { stopReason: "refusal" }, content: "Agent refused this request" }}
    />));
    const refusal = host.querySelector('[data-testid="chat-turn-stop-reason"]')!;
    expect(refusal.textContent).toContain("Agent refused this request");
    expect(refusal.textContent).not.toContain("refusal");
    expect(refusal.querySelector("button")).toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
});
