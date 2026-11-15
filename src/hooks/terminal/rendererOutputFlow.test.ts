import { describe, expect, it, vi } from "vitest";
import { createRendererOutputFlow, rendererFlowEligible } from "./rendererOutputFlow";
import type { NativeOutputFlowUpdate } from "../../lib/nativeBridge";

function fixture() {
  const pending: Array<() => void> = [];
  const updates: NativeOutputFlowUpdate[] = [];
  const send = vi.fn((update: NativeOutputFlowUpdate) => {
    updates.push(update);
    return new Promise<void>(resolve => pending.push(resolve));
  });
  const flow = createRendererOutputFlow("tab", "pty", "attachment", send);
  const complete = async () => { pending.shift()?.(); await Promise.resolve(); };
  return { flow, send, updates, complete };
}

describe("primary renderer output ACKs", () => {
  it("withholds leases throughout startup and permits only ready agents", () => {
    expect(rendererFlowEligible(undefined)).toBe(false);
    expect(rendererFlowEligible({})).toBe(true);
    expect(rendererFlowEligible({ agent_booting: true })).toBe(false);
    expect(rendererFlowEligible({ boot_stage: "reader_ready" })).toBe(false);
    expect(rendererFlowEligible({ boot_stage: "shell_ready" })).toBe(true);
    expect(rendererFlowEligible({ agent_type: "fixture", boot_stage: "provider_ready" })).toBe(false);
    expect(rendererFlowEligible({ agent_type: "fixture", boot_stage: "agent_ready" })).toBe(true);
  });
  it("coalesces a flood into one in-flight ACK and one latest offset", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(10);
    for (let offset = 11; offset <= 10000; offset++) flow.rendered(offset);
    expect(updates).toEqual([{ action: "rendered", tab_id: "tab", pty_id: "pty", attachment: "attachment", offset: 10, attach: true }]);
    await complete();
    expect(updates[updates.length - 1]).toMatchObject({ offset: 10000, attach: false });
    await complete();
    expect(updates).toHaveLength(2);
  });

  it("releases ahead of a new attachment when hide and focus race an ACK", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(10);
    flow.rendered(20);
    flow.suspend();
    flow.rendered(30);
    flow.resume();
    await complete();
    expect(updates[updates.length - 1]).toEqual({ action: "detach", pty_id: "pty", attachment: "attachment" });
    await complete();
    expect(updates[updates.length - 1]).toMatchObject({ offset: 30, attach: true });
    await complete();
  });

  it("never acknowledges hidden writes and starts from the latest completed write on focus", async () => {
    const { flow, updates, complete } = fixture();
    flow.suspend();
    flow.rendered(100);
    await complete();
    expect(updates).toEqual([{ action: "detach", pty_id: "pty", attachment: "attachment" }]);
    flow.resume();
    expect(updates[updates.length - 1]).toMatchObject({ offset: 100, attach: true });
    await complete();
  });

  it("ignores regressing and invalid offsets", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(Number.NaN);
    flow.rendered(-1);
    expect(updates).toHaveLength(0);
    flow.rendered(100);
    flow.rendered(50);
    await complete();
    expect(updates[updates.length - 1]).toMatchObject({ offset: 100 });
    await complete();
  });

  it("stops after a native transport failure instead of building a retry queue", async () => {
    const send = vi.fn().mockRejectedValue(new Error("native session closed"));
    const flow = createRendererOutputFlow("tab", "pty", "id", send);
    flow.rendered(10);
    await Promise.resolve();
    for (let offset = 11; offset < 100; offset++) flow.rendered(offset);
    flow.suspend();
    flow.resume();
    expect(send).toHaveBeenCalledTimes(1);
  });
});
