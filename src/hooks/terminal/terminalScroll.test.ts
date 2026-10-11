import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { Terminal } from "@xterm/xterm";
import { dispatchTerminalTouchScroll, terminalScrollAction } from "./terminalScroll";

class CoordinateWheelEvent extends MouseEvent {
  static readonly DOM_DELTA_LINE = 1;
  readonly deltaMode: number;
  readonly deltaY: number;
  constructor(type: string, init: WheelEventInit = {}) {
    super(type, init);
    this.deltaMode = init.deltaMode ?? 0;
    this.deltaY = init.deltaY ?? 0;
  }
}

// Happy-dom's WheelEvent inherits Event and drops MouseEvent coordinates.
beforeAll(() => vi.stubGlobal("WheelEvent", CoordinateWheelEvent));
afterAll(() => vi.unstubAllGlobals());

describe("terminal gesture routing", () => {
  for (const buffer of ["normal", "alternate"] as const) {
    for (const mouseTracking of ["none", "x10", "vt200", "drag", "any"] as const) {
      it(`${buffer}/${mouseTracking} keeps desktop wheel handling native`, () => {
        expect(terminalScrollAction({ buffer, mouseTracking }, "wheel")).toBe("native");
      });
      it(`${buffer}/${mouseTracking} routes touch by terminal capability`, () => {
        const wheel = buffer === "alternate" || !["none", "x10"].includes(mouseTracking);
        expect(terminalScrollAction({ buffer, mouseTracking }, "touch")).toBe(wheel ? "wheel" : "scrollback");
      });
    }
  }

  it("normal shell/inline TUI scrollback moves the viewport without input", async () => {
    const term = new Terminal({ cols: 80, rows: 3, scrollback: 100 });
    const input: string[] = [];
    term.onData(data => input.push(data));
    await new Promise<void>(resolve => term.write("one\r\ntwo\r\nthree\r\nfour\r\nfive", resolve));
    const bottom = term.buffer.active.viewportY;
    dispatchTerminalTouchScroll(term, -1, 0, 0);
    expect(term.buffer.active.viewportY).toBe(bottom - 1);
    dispatchTerminalTouchScroll(term, 1, 0, 0);
    expect(term.buffer.active.viewportY).toBe(bottom);
    expect(input).toEqual([]);
    term.dispose();
  });

  it("alternate screen has no scrollback and touch reaches xterm's wheel listener", async () => {
    const term = new Terminal({ cols: 80, rows: 3 });
    await new Promise<void>(resolve => term.write("\x1b[?1049h\x1b[?1000h\x1b[?1006h", resolve));
    expect(term.buffer.active.type).toBe("alternate");
    expect(term.buffer.active.baseY).toBe(0);
    expect(term.modes.mouseTrackingMode).toBe("vt200");
    const element = document.createElement("div");
    const screen = document.createElement("div");
    screen.className = "xterm-screen";
    element.append(screen);
    const events: WheelEvent[] = [];
    element.addEventListener("wheel", e => events.push(e as WheelEvent));
    const adapter = {
      buffer: term.buffer,
      modes: term.modes,
      element,
      scrollLines() { throw new Error("alternate screen cannot scroll history"); },
      input() { throw new Error("gestures must use xterm's protocol encoder"); },
    } as unknown as Terminal;
    dispatchTerminalTouchScroll(adapter, -2, 40, 60);
    dispatchTerminalTouchScroll(adapter, 1, 40, 60);
    expect(events.map(e => [e.deltaY, e.deltaMode, e.clientX, e.clientY])).toEqual([
      [-1, WheelEvent.DOM_DELTA_LINE, 40, 60],
      [-1, WheelEvent.DOM_DELTA_LINE, 40, 60],
      [1, WheelEvent.DOM_DELTA_LINE, 40, 60],
    ]);
    term.dispose();
  });
});
