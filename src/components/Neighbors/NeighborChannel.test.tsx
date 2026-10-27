import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useNeighborChannel } from "./useNeighborChannel";
import { NeighborChannel } from "./NeighborChannel";
import type { NeighborRuntimeClient, NeighborListenerEvent } from "./neighborRuntimeClient";

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

async function render(enrolled = true, locked = false, probe = false) {
  let onReception: (event: NeighborListenerEvent) => void = () => {};
  const client: NeighborRuntimeClient = {
    subscribe: vi.fn(async handler => { onReception = handler; return () => {}; }),
    retry: vi.fn(async () => {}),
    status: vi.fn(async () => ({ enrolled, bindings: {} })),
    enroll: vi.fn(async () => {}), rotate: vi.fn(async () => {}),
    permission: vi.fn(async () => ({ allowed: true, scope: "Public status", allow_tab_requests: false })),
    send: vi.fn(async () => ({ message_id: "a".repeat(32), status: "pending" as const })),
    deliveryStatus: vi.fn(async () => ({ status: "delivered" as const })),
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  let channel: ReturnType<typeof useNeighborChannel>;
  function Probe() { channel = useNeighborChannel("owner", "pair", client); return null; }
  await act(async () => root.render(probe ? <Probe /> : <NeighborChannel owner="owner" friendship="pair" locked={locked} client={client} />));
  return Object.assign(client, { state: () => channel, emit: (event: NeighborListenerEvent) => onReception(event) });
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
  expect(JSON.parse(localStorage.getItem("domios.neighbor-deliveries:owner:pair")!)[0].status).toBe("pending");
  await act(async () => button("Check delivery").click());
  expect(client.deliveryStatus).toHaveBeenCalledExactlyOnceWith("a".repeat(32));
  expect(JSON.parse(localStorage.getItem("domios.neighbor-deliveries:owner:pair")!)[0].status).toBe("delivered");
});

it("keeps the Free channel locked", async () => {
  const client = await render(false, true);
  expect(document.querySelector("textarea")).toBeNull();
  expect(document.querySelector("button")).toBeNull();
  expect(client.send).not.toHaveBeenCalled();
  expect(client.enroll).not.toHaveBeenCalled();
});

it("tracks reception state and invokes the retry port", async () => {
  const client = await render(true, false, true);
  for (const status of ["reconnecting", "stopped", "receiving"] as const) {
    await act(async () => client.emit({ friendship_id: "pair", status }));
    expect(client.state().reception).toBe(status);
  }
  await act(async () => client.state().retry());
  expect(client.retry).toHaveBeenCalledExactlyOnceWith("pair");
});

it("preserves a revoked-key reason when checking sender delivery", async () => {
  localStorage.setItem("domios.neighbor-deliveries:owner:pair", JSON.stringify([{ message_id: "a".repeat(32), status: "pending" }]));
  const client = await render();
  vi.mocked(client.deliveryStatus).mockResolvedValue({ status: "not_delivered", reason: "key_revoked" });
  await act(async () => button("Check delivery").click());
  expect(JSON.parse(localStorage.getItem("domios.neighbor-deliveries:owner:pair")!)[0].reason).toBe("key_revoked");
});
