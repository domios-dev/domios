// `codeterm-pane-resize` is broadcast to every mounted pane on every mux-state
// change (tab switch, coalesced PTY output). Only refit when THIS pane's
// container geometry actually changed — otherwise a switch costs O(panes)
// forced reflows + resize_pty IPCs for zero benefit.
export function paneResizeChanged(last: { w: number; h: number }, w: number, h: number): boolean {
  return w >= 20 && h >= 20 && (w !== last.w || h !== last.h);
}
