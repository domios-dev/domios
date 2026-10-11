import type { Terminal } from "@xterm/xterm";
import { captureTerminalViewport, restoreTerminalViewportAfterQueue, type TerminalViewport } from "../../hooks/terminal/terminalViewport";
import { editFrontendScroll, frontendSignals } from "./appState";

export function attachFrontendTerminalViewport(term: Terminal, tabId: string): () => void {
  const saved = frontendSignals.checkpoint().scroll[tabId]?.terminal;
  let active = true;
  let restoring = saved !== null && saved !== undefined;
  if (saved) {
    restoreTerminalViewportAfterQueue(term, JSON.parse(saved) as TerminalViewport, () => active);
    term.write("", () => { restoring = false; });
  }
  const remember = () => {
    if (active && !restoring) editFrontendScroll(tabId, "terminal", JSON.stringify(captureTerminalViewport(term)));
  };
  const scroll = term.onScroll(remember);
  if (!restoring) remember();
  return () => { active = false; scroll.dispose(); };
}
