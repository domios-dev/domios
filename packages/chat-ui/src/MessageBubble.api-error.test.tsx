import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { MessageBubble } from "./MessageBubble";

it("shows the provider error as an alert without expanding a transcript row", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const host = document.createElement("div");
  const root = createRoot(host);
  const content = "Model exo-free has been deprecated";
  try {
    await act(async () => root.render(<MessageBubble showTimestamp={false} message={{ id: "error:own", type: "system", systemSubtype: "api_error", content }} />));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(content);
    expect(host.querySelector('[role="alert"]')?.getAttribute("aria-label")).toBe("Provider API error");
  } finally {
    await act(async () => root.unmount());
  }
});
