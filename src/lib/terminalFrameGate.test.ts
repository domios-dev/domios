import { describe, expect, it } from "vitest";
import { terminalFrameVisible, terminalLayoutRequired, acceptsTerminalOutput } from "./terminalFrameGate";

describe("terminal frame identity", () => {
  it("fits a ready destination before its first identity-confirmed frame can be revealed", () => {
    expect(terminalLayoutRequired(true, true, true)).toBe(true);
    expect(terminalFrameVisible(true, true, false)).toBe(false);
    expect(terminalFrameVisible(true, true, true)).toBe(true);
    expect(terminalLayoutRequired(false, true, true)).toBe(false);
    expect(terminalLayoutRequired(true, false, true)).toBe(false);
    expect(terminalLayoutRequired(true, true, false)).toBe(false);
  });

  it("hides the previous pane immediately and gates the first destination frame", () => {
    expect(terminalFrameVisible(false, true, true)).toBe(false);
    expect(terminalFrameVisible(true, true, false)).toBe(false);
    expect(terminalFrameVisible(true, true, true)).toBe(true);
    expect(terminalFrameVisible(true, false, true)).toBe(false);
  });

  it("accepts only the subscribed tab and its exact PTY", () => {
    expect(acceptsTerminalOutput("a", "pty-a", "b", "pty-a")).toBe(false);
    expect(acceptsTerminalOutput("a", "pty-a", "a", "pty-b")).toBe(false);
    expect(acceptsTerminalOutput("a", "pty-a", undefined, "pty-a")).toBe(false);
    expect(acceptsTerminalOutput("a", "pty-a", "a", undefined)).toBe(false);
    expect(acceptsTerminalOutput("a", undefined, "a", "pty-a")).toBe(true);
    expect(acceptsTerminalOutput("a", "pty-a", "a", "pty-a")).toBe(true);
  });
});
