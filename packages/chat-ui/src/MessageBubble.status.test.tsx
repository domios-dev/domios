import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { MessageBubble } from "./MessageBubble";

const raw = "Opaque quota exhausted for this key. Retry later.\nhttps://opaque.example/quota\nmetric: opaque/requests";

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
