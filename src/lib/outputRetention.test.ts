import { describe, expect, it } from "vitest";
import { OutputRetention } from "./outputRetention";

describe("retained terminal delta", () => {
  const cursor = { boot: "boot", ptyId: "pty", offset: 3 };
  it("allocates only unseen bytes across chunk boundaries", () => {
    const retained = new OutputRetention();
    retained.append("a", { ...cursor, bytes: new Uint8Array([1, 2, 3]) });
    retained.append("a", { ...cursor, offset: 6, bytes: new Uint8Array([4, 5, 6]) });
    expect([...retained.snapshot("a", { ...cursor, offset: 2 })!.bytes]).toEqual([3, 4, 5, 6]);
    expect(retained.snapshot("a", { ...cursor, offset: 6 })!.bytes).toHaveLength(0);
    expect(retained.snapshot("b", cursor)).toBeUndefined();
  });
  it("ignores foreign boot and PTY cursors and preserves the retained gap", () => {
    const retained = new OutputRetention(2);
    retained.append("a", { ...cursor, bytes: new Uint8Array([1, 2, 3]) });
    expect([...retained.snapshot("a", { ...cursor, boot: "other" })!.bytes]).toEqual([2, 3]);
    expect([...retained.snapshot("a", { ...cursor, ptyId: "other" })!.bytes]).toEqual([2, 3]);
    expect(retained.snapshot("a", { ...cursor, offset: 0 })!.offset).toBe(3);
  });
});
