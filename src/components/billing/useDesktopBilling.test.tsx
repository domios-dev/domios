import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useDesktopBilling } from "./useDesktopBilling";
import { desktopCatalogFixture } from "./desktopBilling.testdata";

const transport = vi.hoisted(() => ({ localFetch: vi.fn() }));
const bridge = vi.hoisted(() => ({ handlers: new Set<() => void>() }));
vi.mock("../../lib/bridge", () => ({ listen: async (_event: string, handler: () => void) => {
  bridge.handlers.add(handler);
  return () => { bridge.handlers.delete(handler); };
} }));
vi.mock("../../lib/webApi", () => transport);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
let state: ReturnType<typeof useDesktopBilling>;
function Harness({ identity = "owner", enabled = true }: { identity?: string; enabled?: boolean }) { state = useDesktopBilling(identity, enabled); return null; }
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  transport.localFetch.mockReset();
  bridge.handlers.clear();
  vi.restoreAllMocks();
});
async function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<Harness />));
}
const readback = { billing_interval: "month", current_tier: "max", paid_subscription: { tier: "max", status: "active", period_end: 1700000000 }, period_end: 1700000000, pending_change: null };
const ok = (value: unknown) => new Response(JSON.stringify(value), { status: 200 });
const setup = (change: () => Promise<Response>) => transport.localFetch.mockImplementation(async (_url: string, init: RequestInit) => {
  const { operation } = JSON.parse(String(init.body)) as { operation: string };
  return ["catalog", "refresh_catalog"].includes(operation) ? ok(desktopCatalogFixture) : ["subscription", "refresh_subscription"].includes(operation) ? ok(readback) : change();
});

it("keeps an unconfirmed payable request from being submitted again", async () => {
  const change = vi.fn(async () => { throw new Error("disconnected"); });
  setup(change);
  await mount();
  await act(async () => { expect(await state.change("pri_pro")).toBe(false); });
  await act(async () => { expect(await state.change("pri_pro")).toBe(false); state.refresh(); });
  await act(async () => { expect(await state.change("pri_pro")).toBe(false); });
  expect(change).toHaveBeenCalledTimes(1);
  expect(state.subscription).toEqual(readback);
  expect(state.submitted).toBe(true);
});

it("shows the server's staff-only sandbox refusal honestly", async () => {
  const change = vi.fn(async () => new Response("test_billing_restricted: opaque-refusal-detail", { status: 503 }));
  setup(change);
  await mount();
  await act(async () => { await state.change("pri_pro"); });
  expect(state.subscription).toEqual(readback);
  expect(state.submitted).toBe(false);
  await act(async () => { await state.change("pri_pro"); });
  expect(change).toHaveBeenCalledTimes(2);
});

it("does not publish an old identity's pending change into a new account", async () => {
  let complete!: (response: Response) => void;
  setup(() => new Promise<Response>(resolve => { complete = resolve; }));
  await mount();
  let change!: Promise<boolean>;
  await act(async () => { change = state.change("pri_pro"); });
  await act(async () => mounted!.root.render(<Harness identity="other" />));
  await act(async () => { complete(ok({ status: "scheduled", target_plan: "pro" })); expect(await change).toBe(false); });
  expect(state.subscription?.pending_change).toBeNull();
  expect(state.submitted).toBe(false);
});

it("defers closed consumers, shares reads on open and never fetches on focus", async () => {
  let now = Date.now() + 120_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  setup(async () => ok({}));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<Harness identity="shared-owner" enabled={false} />));
  expect(transport.localFetch).not.toHaveBeenCalled();
  await act(async () => root.render(<><Harness identity="shared-owner" /><Harness identity="shared-owner" /></>));
  expect(transport.localFetch).toHaveBeenCalledTimes(2);
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(transport.localFetch).toHaveBeenCalledTimes(2);
  now += 60_000;
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(transport.localFetch).toHaveBeenCalledTimes(2);
  await act(async () => root.render(<><Harness identity="shared-owner" /><Harness identity="shared-other" /></>));
  expect(transport.localFetch).toHaveBeenCalledTimes(4);
});

it("updates two mounted consumers from one entitlement event and replaces a stale pending answer", async () => {
  const seen: ReturnType<typeof useDesktopBilling>[] = [];
  function Consumer({ index }: { index: number }) {
    seen[index] = useDesktopBilling("event-owner");
    return null;
  }
  let finishOld!: (response: Response) => void;
  let finishFresh!: (response: Response) => void;
  let refreshed!: () => void;
  const started = new Promise<void>(resolve => { refreshed = resolve; });
  let subscriptionCalls = 0;
  let catalogCalls = 0;
  transport.localFetch.mockImplementation(async (_url: string, init: RequestInit) => {
    const { operation } = JSON.parse(String(init.body)) as { operation: string };
    if (operation === "catalog") { catalogCalls++; return ok(desktopCatalogFixture); }
    subscriptionCalls++;
    if (subscriptionCalls === 1) return new Promise<Response>(resolve => { finishOld = resolve; });
    refreshed();
    return new Promise<Response>(resolve => { finishFresh = resolve; });
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<><Consumer index={0} /><Consumer index={1} /></>));
  expect(bridge.handlers.size).toBe(1);
  await act(async () => { for (const handler of bridge.handlers) handler(); });
  expect(subscriptionCalls).toBe(1);
  await act(async () => {
    finishOld(ok({ ...readback, current_tier: "old-tier" }));
    await started;
  });
  expect(seen.map(value => value.subscription)).toEqual([null, null]);
  await act(async () => finishFresh(ok(readback)));
  expect(seen.map(value => value.subscription)).toEqual([readback, readback]);
  expect(subscriptionCalls).toBe(2);
  expect(catalogCalls).toBe(1);
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(subscriptionCalls).toBe(2);
  expect(catalogCalls).toBe(1);
});
