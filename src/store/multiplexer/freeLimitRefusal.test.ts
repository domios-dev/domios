import { describe, expect, it, vi } from "vitest";
import { FREE_LIMIT_REACHED, freeLimitMessage, freeLimitToast } from "./freeLimitRefusal";

const reason = "opaque-refusal-body";
const coded = `${FREE_LIMIT_REACHED}: ${reason}`;

describe("free-tier refusal", () => {
  it("reads the coded refusal from a Tauri string, an Error, and a prefixed route message", () => {
    expect(freeLimitMessage(coded)).toBe(reason);
    expect(freeLimitMessage(new Error(coded))).toBe(reason);
    expect(freeLimitMessage(`Failed to spawn: ${coded}`)).toBe(reason);
  });

  it("ignores every other error", () => {
    expect(freeLimitMessage("workspace name must not be empty")).toBeNull();
    expect(freeLimitMessage(new Error(`${FREE_LIMIT_REACHED}: `))).toBeNull();
    expect(freeLimitMessage(undefined)).toBeNull();
    expect(freeLimitToast("plain failure", "ws", async () => {})).toBeNull();
  });

  it("raises one replaceable toast per workspace whose action upgrades", async () => {
    const upgrade = vi.fn(async () => {});
    const toast = freeLimitToast(coded, "ws_1", upgrade, 7)!;
    expect(toast).toMatchObject({ workspace_id: "ws_1", body: reason, level: "action_needed", sticky: true, timestamp: 7 });
    expect(freeLimitToast(new Error(coded), "ws_1", upgrade)!.id).toBe(toast.id);
    expect(freeLimitToast(coded, null, upgrade)!.id).not.toBe(toast.id);
    expect(toast.local_actions).toHaveLength(1);
    await toast.local_actions![0].run();
    expect(upgrade).toHaveBeenCalledOnce();
  });
});
