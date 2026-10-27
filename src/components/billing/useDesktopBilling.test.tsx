import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useDesktopBilling } from "./useDesktopBilling";
import { desktopCatalogFixture } from "./desktopBilling.testdata";

const transport = vi.hoisted(() => ({ localFetch: vi.fn() }));
vi.mock("../../lib/webApi", () => transport);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
let state: ReturnType<typeof useDesktopBilling>;
function Harness({ identity = "owner" }: { identity?: string }) { state = useDesktopBilling(identity); return null; }
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  transport.localFetch.mockReset();
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
  return operation === "catalog" ? ok(desktopCatalogFixture) : operation === "subscription" ? ok(readback) : change();
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
