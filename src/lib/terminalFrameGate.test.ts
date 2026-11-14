import { describe, expect, it } from "vitest";
import { terminalFrameVisible, terminalLayoutRequired, acceptsTerminalOutput, outputBindingPtyId, terminalContentTopInset } from "./terminalFrameGate";

describe("terminal content below the tab top bar", () => {
  it("reserves the top bar's overhang so PTY row 0 starts below it in plain terminal mode", () => {
    expect(terminalContentTopInset({ terminalMode: true, touchUi: false, fallbackChat: false, keyBar: false }))
      .toBe("var(--ct-top-bar-fade, 12px)");
  });

  it("keeps the larger touch inset and the fallback frame's own offsets", () => {
    expect(terminalContentTopInset({ terminalMode: true, touchUi: true, fallbackChat: false, keyBar: true }))
      .toBe("calc(1.25rem + var(--ct-safe-top, 0px))");
    expect(terminalContentTopInset({ terminalMode: false, touchUi: false, fallbackChat: true, keyBar: false })).toBe(8);
    expect(terminalContentTopInset({ terminalMode: false, touchUi: false, fallbackChat: false, keyBar: false })).toBe(0);
  });
});

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

  it("binds a remote terminal to the peer's own PTY, never to the composite id it routes by", () => {
    expect(outputBindingPtyId("local", "pty-a", "ignored")).toBe("pty-a");
    expect(outputBindingPtyId("mesh:peer/tab", "mesh:peer/tab", "peer-pty")).toBe("peer-pty");
    expect(outputBindingPtyId("mesh:peer/tab", "mesh:peer/tab", null)).toBeUndefined();
    const bound = outputBindingPtyId("mesh:peer/tab", "mesh:peer/tab", "peer-pty");
    expect(acceptsTerminalOutput("tab", bound, "tab", "peer-pty")).toBe(true);
    expect(acceptsTerminalOutput("tab", bound, "tab", "older-peer-pty")).toBe(false);
  });
});
