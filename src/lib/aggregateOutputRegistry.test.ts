import { describe, expect, it, vi } from "vitest";
import { createAggregateOutputRegistry } from "./aggregateOutputRegistry";
import { OutputRetention } from "./outputRetention";

const frame = (bytes: number[], offset: number, ptyId = "pty", boot = "boot") => ({ bytes: new Uint8Array(bytes), offset, ptyId, boot });

describe("page aggregate output ownership", () => {
  it("uses one connection for N terminal attachments and ignores recovery while live", () => {
    const close = vi.fn();
    const connect = vi.fn(() => close);
    const registry = createAggregateOutputRegistry(connect);
    for (let terminal = 0; terminal < 30; terminal++) { registry.attach(); registry.recover(); }
    expect(connect).toHaveBeenCalledTimes(1);
    expect(close).not.toHaveBeenCalled();
    registry.dispose();
    expect(close).toHaveBeenCalledTimes(1);
    registry.recover();
    expect(connect).toHaveBeenCalledTimes(1);
  });
  it("waits for explicit recovery after EOF or failure and replaces one connection", () => {
    const close = vi.fn();
    const connect = vi.fn(() => close);
    const registry = createAggregateOutputRegistry(connect);
    registry.attach();
    registry.ended();
    expect(connect).toHaveBeenCalledTimes(1);
    registry.recover();
    registry.recover();
    expect(connect).toHaveBeenCalledTimes(2);
    const replacement = vi.fn(() => () => {});
    registry.replace(replacement);
    expect(close).toHaveBeenCalledTimes(1);
    expect(replacement).toHaveBeenCalledTimes(1);
    registry.attach();
    expect(replacement).toHaveBeenCalledTimes(1);
  });
});

describe("retained aggregate output", () => {
  it("retains exact raw bytes per tab, deduplicates overlaps and separates PTY rings", () => {
    const cache = new OutputRetention();
    cache.append("a", frame([0xff, 0xe2], 2));
    cache.append("a", frame([0xe2, 0x82, 0xac], 4));
    cache.append("a", frame([0xe2, 0x82, 0xac], 4));
    cache.append("b", frame([10], 1));
    expect(Array.from(cache.snapshot("a")!.bytes)).toEqual([0xff, 0xe2, 0x82, 0xac]);
    expect(Array.from(cache.snapshot("b")!.bytes)).toEqual([10]);
    cache.append("a", frame([5], 1, "new-pty"));
    expect(cache.snapshot("a")).toEqual(frame([5], 1, "new-pty"));
  });
  it("bounds retained history, preserves byte offsets and drops gaps and prior boots", () => {
    const cache = new OutputRetention(3);
    cache.append("a", frame([1, 2, 3], 3));
    cache.append("a", frame([4, 5], 5));
    expect(cache.snapshot("a")).toEqual(frame([3, 4, 5], 5));
    cache.append("a", frame([8], 8));
    expect(cache.snapshot("a")).toEqual(frame([8], 8));
    cache.append("a", frame([9], 1, "pty", "new"));
    expect(cache.snapshot("a")).toEqual(frame([9], 1, "pty", "new"));
    cache.clear();
    expect(cache.snapshot("a")).toBeUndefined();
  });
});
