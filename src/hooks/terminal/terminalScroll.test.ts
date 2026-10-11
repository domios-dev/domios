import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { Terminal } from "@xterm/xterm";
import { dispatchTerminalTouchScroll, terminalScrollAction, terminalScrollPoints } from "./terminalScroll";

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
  it("distributes wheel positions across the dragged region in both directions", () => {
    expect(terminalScrollPoints(-3, { x: 10, y: 40 }, { x: 40, y: 100 })).toEqual([
      { x: 20, y: 60 }, { x: 30, y: 80 }, { x: 40, y: 100 },
    ]);
    expect(terminalScrollPoints(3, { x: 40, y: 100 }, { x: 10, y: 40 })).toEqual([
      { x: 30, y: 80 }, { x: 20, y: 60 }, { x: 10, y: 40 },
    ]);
    expect(terminalScrollPoints(0, { x: 10, y: 40 }, { x: 40, y: 100 })).toEqual([]);
  });
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

it("touch produces SGR wheel bytes through real xterm and the terminal PTY writer", async () => {
  const offsetWidth = vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("xterm-char-measure-element") ? 8 * (this.textContent?.length ?? 32) : 640;
  });
  const offsetHeight = vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(16);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const { useTouchScroll } = await import("./useTouchScroll");
  const { useTerminalInput } = await import("./useTerminalInput");
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const host = document.createElement("div");
  const reactHost = document.createElement("div");
  document.body.append(host, reactHost);
  Object.defineProperty(host, "clientHeight", { value: 384 });
  const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
  term.open(host);
  const screen = host.querySelector<HTMLElement>(".xterm-screen")!;
  screen.style.padding = "0px";
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return undefined as T;
  });
  const root = createRoot(reactHost);
  const options = { containerRef: { current: host }, termRef: { current: term }, termReady: true };
  function Harness() {
    useTouchScroll(options);
    useTerminalInput({
      ...options,
      tabId: "touch-tab",
      connectedPtyRef: { current: "touch-pty" },
      terminalActive: true,
      onTabKey: () => false,
      onNonTabKey() {},
      onInputData() {},
    });
    return null;
  }
  function touch(type: string, y: number) {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "touches", { value: [{ clientX: 80, clientY: y }] });
    screen.dispatchEvent(event);
  }
  try {
    await act(async () => root.render(createElement(Harness)));
    await new Promise<void>(resolve => term.write("\x1b[?1049h\x1b[?1003h\x1b[?1006h", resolve));
    expect(term.modes.mouseTrackingMode).toBe("any");
    await act(async () => {
      touch("touchstart", 180);
      touch("touchmove", 196);
    });
    expect(calls).toEqual([{
      command: "write_pty_string",
      args: { ptyId: "touch-pty", data: "\x1b[<64;11;13M" },
    }]);
    calls.length = 0;
    await act(async () => {
      touch("touchend", 196);
      touch("touchstart", 64);
      touch("touchmove", 352);
      touch("touchend", 352);
      touch("touchstart", 352);
      touch("touchmove", 64);
      touch("touchend", 64);
    });
    expect(calls.every(call => call.command === "write_pty_string" && call.args?.ptyId === "touch-pty")).toBe(true);
    expect(calls.map(call => call.args?.data).join("")).toBe([
      ...Array.from({ length: 18 }, (_, i) => `\x1b[<64;11;${i + 6}M`),
      ...Array.from({ length: 18 }, (_, i) => `\x1b[<65;11;${22 - i}M`),
    ].join(""));
  } finally {
    await act(async () => root.unmount());
    term.dispose();
    host.remove();
    reactHost.remove();
    __setInvokeForTest(null);
    offsetWidth.mockRestore();
    offsetHeight.mockRestore();
  }
});
