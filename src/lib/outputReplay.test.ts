import { describe, expect, it } from "vitest";
import { planOutputReplay, TerminalOutputReplay } from "./outputReplay";

const frame = (bytes: number[], offset: number, boot = "boot", ptyId = "pty") => ({
  bytes: new Uint8Array(bytes), offset, boot, ptyId,
});

describe("terminal output attach replay", () => {
  it("delivers exact bytes once to a retained terminal after duplicate and overlapping replay", () => {
    const replay = new TerminalOutputReplay();
    const terminal = {};
    const bytes: number[] = [];
    const write = (chunk: Uint8Array) => bytes.push(...chunk);
    replay.deliver(terminal, "tab", frame([0x1b, 0xff, 0xe2], 3), write);
    replay.deliver(terminal, "tab", frame([0x1b, 0xff, 0xe2], 3), write);
    replay.deliver(terminal, "tab", frame([0xe2, 0x82, 0xac], 5), write);
    expect(bytes).toEqual([0x1b, 0xff, 0xe2, 0x82, 0xac]);
    expect(replay.offset(terminal, "tab", "boot", "pty")).toBe(5);
  });

  it("requests full retained history for a new terminal while retained terminals resume", () => {
    const replay = new TerminalOutputReplay();
    const desktop = {}, browser = {}, remounted = {};
    const outputs = [[], [], []] as number[][];
    for (const [index, terminal] of [desktop, browser].entries()) {
      replay.deliver(terminal, "tab", frame([1, 2, 3], 3), (bytes) => outputs[index].push(...bytes));
    }
    expect(replay.offset(remounted, "tab", "boot", "pty")).toBe(0);
    for (const [index, terminal] of [desktop, browser, remounted].entries()) {
      replay.deliver(terminal, "tab", frame([1, 2, 3, 4], 4), (bytes) => outputs[index].push(...bytes));
    }
    expect(outputs).toEqual([[1, 2, 3, 4], [1, 2, 3, 4], [1, 2, 3, 4]]);
  });

  it("does not commit a cursor if enqueue fails", () => {
    const replay = new TerminalOutputReplay(), terminal = {};
    expect(() => replay.deliver(terminal, "tab", frame([1], 1), () => { throw new Error("detached"); })).toThrow("detached");
    expect(replay.offset(terminal, "tab", "boot", "pty")).toBe(0);
  });

  it("resets namespaces on daemon boot and PTY replacement, with retained-ring gaps", () => {
    const cursor = { boot: "boot", ptyId: "pty", offset: 99 };
    expect([...planOutputReplay(cursor, frame([7, 8], 2, "next")).bytes]).toEqual([7, 8]);
    expect([...planOutputReplay(cursor, frame([7, 8], 2, "boot", "next")).bytes]).toEqual([7, 8]);
    expect([...planOutputReplay({ ...cursor, offset: 1 }, frame([7, 8], 10)).bytes]).toEqual([7, 8]);
    expect([...planOutputReplay(cursor, frame([7, 8], 10)).bytes]).toEqual([]);
  });
});
