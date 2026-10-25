import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { NeighborsInventory } from "./NeighborsSection";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";

const notices = vi.hoisted(() => ({ publishNotice: vi.fn() }));
vi.mock("../../lib/notices", () => notices);
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: { kind: "signed_out" } }) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const neighbor = { uid: "peer", displayName: "Ada", photoURL: null, friendship_id: "pair", inbound_per_hour: 10, outbound_per_hour: 20, since: 1700000000 };
const request = { uid: "requester", displayName: "Grace", photoURL: null, request_id: "request", created_at: 1700000000 };
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  localStorage.clear();
  notices.publishNotice.mockClear();
});

function button(label: string): HTMLButtonElement {
  const found = Array.from(document.querySelectorAll("button")).find(element => element.textContent === label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}

async function render(access: NeighborList["access"] = "paid") {
  const list: NeighborList = { schema_version: 1, access, friends: [neighbor], incoming: [request] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  const onOpenGeneral = vi.fn();
  await act(async () => root.render(<NeighborsInventory identity="owner" client={client} onOpenGeneral={onOpenGeneral} />));
  await act(async () => button("Neighbors").click());
  return { client, list, onOpenGeneral };
}

it("opens and collapses the registry and notifies once for a new request", async () => {
  const { client } = await render();
  expect(button("Neighbors").getAttribute("aria-expanded")).toBe("true");
  expect(notices.publishNotice).toHaveBeenCalledTimes(1);
  await act(async () => button("Refresh Neighbors").click());
  expect(client.list).toHaveBeenCalledTimes(2);
  expect(notices.publishNotice).toHaveBeenCalledTimes(1);
  await act(async () => button("Neighbors").click());
  expect(button("Neighbors").getAttribute("aria-expanded")).toBe("false");
});

it("saves an email request once and never claims email delivery", async () => {
  const { client } = await render();
  await act(async () => button("Add Neighbor").click());
  const input = document.querySelector<HTMLInputElement>('input[type="email"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "friend@example.test");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
  expect(client.apply).toHaveBeenCalledExactlyOnceWith({ operation: "request", email: "friend@example.test" });
  expect(button("Send request or invitation").disabled).toBe(true);
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain("Request or invitation saved.");
});

it("lets Free accept and manage dormant friendships while initiation offers plans", async () => {
  const { client } = await render("locked");
  expect(document.body.textContent).not.toContain("Add Neighbor");
  expect(button("Upgrade to add Neighbors")).toBeDefined();
  await act(async () => button("Accept Grace").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "respond", request_id: "request", accept: true });
  await act(async () => button("Ada").click());
  const input = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "42");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Save limit").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "set_limit", friendship_id: "pair", per_hour: 42 });
  await act(async () => button("Remove Neighbor").click());
  expect(client.apply).not.toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
  await act(async () => button("Confirm remove").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
});

it("links the permissions hint to the General Agent", async () => {
  const { onOpenGeneral } = await render();
  await act(async () => button("Ada").click());
  await act(async () => button("Open General Agent").click());
  expect(onOpenGeneral).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});
