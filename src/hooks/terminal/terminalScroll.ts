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

export function dispatchTerminalTouchScroll(term: Terminal, lines: number, x: number, y: number): void {
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
  for (let row = 0; row < Math.abs(lines); row++) {
    screen.dispatchEvent(new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaMode: WheelEvent.DOM_DELTA_LINE,
      deltaY: Math.sign(lines),
      clientX: x,
      clientY: y,
    }));
  }
}
