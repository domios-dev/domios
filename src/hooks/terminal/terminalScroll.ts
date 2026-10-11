import type { Terminal } from "@xterm/xterm";

export interface TerminalScrollState {
  buffer: "normal" | "alternate";
  mouseTracking: Terminal["modes"]["mouseTrackingMode"];
}

export function terminalScrollAction(state: TerminalScrollState, gesture: "touch" | "wheel") {
  if (gesture === "wheel") return "native";
  const reportsWheel = state.mouseTracking !== "none" && state.mouseTracking !== "x10";
  return state.buffer === "alternate" || reportsWheel ? "wheel" : "scrollback";
}

export interface TerminalScrollPoint {
  x: number;
  y: number;
}

export function terminalScrollPoints(lines: number, start: TerminalScrollPoint, end: TerminalScrollPoint): TerminalScrollPoint[] {
  return Array.from({ length: Math.abs(lines) }, (_, index) => {
    const fraction = (index + 1) / Math.abs(lines);
    return {
      x: start.x + (end.x - start.x) * fraction,
      y: start.y + (end.y - start.y) * fraction,
    };
  });
}

export function dispatchTerminalTouchScroll(term: Terminal, lines: number, x: number, y: number, start = { x, y }): void {
  const action = terminalScrollAction({
    buffer: term.buffer.active.type,
    mouseTracking: term.modes.mouseTrackingMode,
  }, "touch");
  if (action === "scrollback") {
    term.scrollLines(lines);
    return;
  }
  const screen = term.element?.querySelector(".xterm-screen");
  if (!screen) return;
  for (const point of terminalScrollPoints(lines, start, { x, y })) {
    screen.dispatchEvent(new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaMode: WheelEvent.DOM_DELTA_LINE,
      deltaY: Math.sign(lines),
      clientX: point.x,
      clientY: point.y,
    }));
  }
}
