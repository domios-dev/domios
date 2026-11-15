import { describe, expect, it, vi } from "vitest";
import { createRendererOutputFlow, rendererFlowEligible, OUTPUT_ACK_BYTES } from "./rendererOutputFlow";
import type { NativeOutputFlowUpdate } from "../../lib/nativeBridge";

function fixture() {
  const pending: Array<(accepted: boolean) => void> = [];
  const updates: NativeOutputFlowUpdate[] = [];
  const send = vi.fn((update: NativeOutputFlowUpdate) => {
    updates.push(update);
    return new Promise<boolean>(resolve => pending.push(resolve));
  });
  const flow = createRendererOutputFlow("tab", "pty", "attachment", send);
  const complete = async (accepted = true) => { pending.shift()?.(accepted); await Promise.resolve(); };
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
    for (let offset = 11; offset <= OUTPUT_ACK_BYTES + 10; offset++) flow.rendered(offset);
    expect(updates).toEqual([{ action: "rendered", tab_id: "tab", pty_id: "pty", attachment: "attachment", offset: 10, attach: true }]);
    await complete();
    expect(updates[updates.length - 1]).toMatchObject({ offset: OUTPUT_ACK_BYTES + 10, attach: false });
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
    expect(updates).toHaveLength(1);
    expect(updates[0]).toMatchObject({ offset: 100 });
  });

  it("releases a failed attachment and retries only on a later render event", async () => {
    const send = vi.fn().mockRejectedValueOnce(new Error("QueueFull")).mockResolvedValue(true);
    const flow = createRendererOutputFlow("tab", "pty", "id", send);
    flow.rendered(10);
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    expect(send.mock.calls.map(call => call[0].action)).toEqual(["rendered", "detach"]);
    flow.rendered(10 + OUTPUT_ACK_BYTES);
    await Promise.resolve();
    expect(send.mock.calls[2][0]).toMatchObject({ action: "rendered", attach: true, offset: 10 + OUTPUT_ACK_BYTES });
  });

  it("coalesces small completed writes without repeated focus/activity IPC", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(0); await complete();
    for (let offset = 1; offset < OUTPUT_ACK_BYTES; offset++) { flow.rendered(offset); flow.resume(); }
    expect(updates).toHaveLength(1);
    flow.rendered(OUTPUT_ACK_BYTES);
    expect(updates[1]).toMatchObject({ offset: OUTPUT_ACK_BYTES, attach: false });
    await complete();
  });

  it("retries attach after an authoritative native focus event rejects an early DOM focus", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(10); await complete(false);
    flow.refresh();
    expect(updates[1]).toMatchObject({ offset: 10, attach: true });
    await complete();
  });

  it("bounds rejected attachment traffic until byte progress or a real focus event", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(0); await complete(false);
    for (let offset = 1; offset < OUTPUT_ACK_BYTES; offset++) { flow.rendered(offset); flow.resume(); }
    expect(updates).toHaveLength(1);
    flow.refresh();
    expect(updates[1]).toMatchObject({ offset: OUTPUT_ACK_BYTES - 1, attach: true });
    await complete();
  });

  it("clears the old boot offset and ignores an old in-flight attachment on reset", async () => {
    const { flow, updates, complete } = fixture();
    flow.rendered(1000000);
    flow.reset();
    flow.rendered(1);
    flow.resume();
    await complete();
    expect(updates[1]).toMatchObject({ action: "detach" });
    await complete();
    expect(updates[2]).toMatchObject({ offset: 1, attach: true });
    await complete();
  });
});
