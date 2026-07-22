import type { Terminal } from "@xterm/xterm";

type ViewportTerminal = Pick<Terminal, "scrollToBottom" | "scrollToLine"> & {
  buffer: {
    active: Pick<Terminal["buffer"]["active"], "type" | "viewportY" | "baseY">;
  };
};

type QueueableViewportTerminal = ViewportTerminal & Pick<Terminal, "write">;

export interface TerminalViewport {
  followOutput: boolean;
  line: number;
}

export function captureTerminalViewport(term: ViewportTerminal): TerminalViewport {
  const buffer = term.buffer.active;
  return {
    followOutput: buffer.type === "alternate" || buffer.viewportY >= buffer.baseY,
    line: buffer.viewportY,
  };
}

export function restoreTerminalViewport(term: ViewportTerminal, viewport: TerminalViewport): void {
  if (viewport.followOutput) {
    term.scrollToBottom();
    return;
  }
  term.scrollToLine(Math.min(viewport.line, term.buffer.active.baseY));
}

export function restoreTerminalViewportAfterQueue(
  term: QueueableViewportTerminal,
  viewport: TerminalViewport,
  isCurrent: () => boolean,
): void {
  term.write("", () => {
    if (isCurrent()) restoreTerminalViewport(term, viewport);
  });
}
