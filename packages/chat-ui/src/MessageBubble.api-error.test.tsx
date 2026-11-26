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

it("renders structured provider-error remediation with copyable commands, Retry, and collapsed Details", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const host = document.createElement("div");
  const root = createRoot(host);
  let retries = 0;
  const remediation = {
    code: "api_key_expired",
    title: "Stored key has expired",
    message: "Sign in again or remove the stored key to continue anonymously.",
    commands: [
      { label: "Sign in again", command: "provider auth login" },
      { label: "Remove stored key", command: "provider auth logout" },
    ],
    retry: true,
    rawDetail: "HTTP 401 authentication_error: api_key_expired",
  };
  try {
    await act(async () => root.render(
      <MessageBubble
        showTimestamp={false}
        onContinueTurn={() => { retries += 1; }}
        message={{ id: "error:remediated", type: "system", systemSubtype: "api_error", content: remediation.message, toolInput: { remediation } }}
      />,
    ));
    const alert = host.querySelector('[data-testid="provider-error-notice"]')!;
    expect(alert.getAttribute("role")).toBe("alert");
    expect(alert.getAttribute("data-error-code")).toBe("api_key_expired");
    expect(alert.textContent).toContain(remediation.title);
    expect(alert.textContent).toContain(remediation.message);
    expect(alert.textContent).toContain("provider auth login");
    expect(alert.textContent).toContain("provider auth logout");
    expect(alert.querySelectorAll('button[aria-label*="Copy"], button[title*="Copy"]').length).toBeGreaterThanOrEqual(2);
    const retry = alert.querySelector<HTMLButtonElement>('[data-testid="provider-error-retry"]')!;
    expect(retry.textContent).toBe("Retry");
    await act(async () => retry.click());
    expect(retries).toBe(1);
    const details = alert.querySelector("details")!;
    expect(details.open).toBe(false);
    expect(details.querySelector("pre")?.textContent).toBe(remediation.rawDetail);
  } finally {
    await act(async () => root.unmount());
  }
});

it("an archived conversation that cannot be read is one calm line with its raw cause in collapsed Details", async () => {
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true });
  const host = document.createElement("div");
  const root = createRoot(host);
  const content = "opaque daemon chain: opaque_code: opaque-session-id";
  try {
    await act(async () => root.render(<MessageBubble showTimestamp={false}
      message={{ id: "archived-session-unavailable-opaque", type: "system", systemSubtype: "archived_session_unavailable", content }} />));
    const row = host.querySelector('[data-testid="chat-archived-unavailable"]')!;
    expect(row.querySelector("p")?.textContent).toBe("Earlier messages in this tab aren't available.");
    const details = row.querySelector("details")!;
    expect(details.open).toBe(false);
    expect(details.querySelector("pre")?.textContent).toBe(content);
    const visible = (row.textContent ?? "").replace(details.querySelector("pre")!.textContent ?? "", "");
    expect(visible).not.toContain("archived_session_unavailable");
    expect(visible).not.toContain("opaque");
  } finally {
    await act(async () => root.unmount());
  }
});
