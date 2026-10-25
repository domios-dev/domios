import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { NeighborChannel } from "./NeighborChannel";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  localStorage.clear();
});

function button(label: string): HTMLButtonElement {
  const found = Array.from(document.querySelectorAll("button")).find(element => element.textContent === label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}

async function render(enrolled = true, locked = false) {
  const client: NeighborRuntimeClient = {
    status: vi.fn(async () => ({ enrolled, bindings: {} })),
    enroll: vi.fn(async () => {}), rotate: vi.fn(async () => {}),
    permission: vi.fn(async () => ({ allowed: true, scope: "Public status", allow_tab_requests: false })),
    send: vi.fn(async () => ({ message_id: "a".repeat(32), status: "pending" as const })),
    deliveryStatus: vi.fn(async () => "delivered" as const),
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborChannel owner="owner" friendship="pair" locked={locked} client={client} />));
  return client;
}

it("requires a deliberate machine takeover confirmation", async () => {
  const client = await render(false);
  await act(async () => button("Enable encrypted channel").click());
  expect(client.enroll).not.toHaveBeenCalled();
  await act(async () => button("Cancel").click());
  expect(client.enroll).not.toHaveBeenCalled();
  await act(async () => button("Enable encrypted channel").click());
  await act(async () => button("Confirm this machine").click());
  expect(client.enroll).toHaveBeenCalledTimes(1);
});

it("sends once, clears the composer and stores only delivery metadata", async () => {
  const client = await render();
  const input = document.querySelector("textarea")!;
  const body = "opaque-private-content";
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, body);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Send encrypted message").click());
  expect(client.send).toHaveBeenCalledExactlyOnceWith("pair", body);
  expect(input.value).toBe("");
  expect(localStorage.getItem("domios.neighbor-deliveries:owner:pair")).not.toContain(body);
  expect(document.body.textContent).toContain("Pending");
  await act(async () => button("Check delivery").click());
  expect(client.deliveryStatus).toHaveBeenCalledExactlyOnceWith("a".repeat(32));
  expect(document.body.textContent).toContain("Delivered");
});

it("keeps the Free channel locked", async () => {
  const client = await render(false, true);
  expect(document.querySelector("textarea")).toBeNull();
  expect(document.body.textContent).not.toContain("Enable encrypted channel");
  expect(client.send).not.toHaveBeenCalled();
  expect(client.enroll).not.toHaveBeenCalled();
});
