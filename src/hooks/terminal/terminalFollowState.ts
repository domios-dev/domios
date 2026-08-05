import { captureTerminalViewport } from "./terminalViewport";

type FollowStateTerminal = Parameters<typeof captureTerminalViewport>[0];

/** A jump-to-bottom control is only meaningful while the viewport is detached from
 *  the tail. An alternate-screen TUI has no scrollback to be detached from, which
 *  captureTerminalViewport already treats as following. */
export function shouldOfferScrollToBottom(term: FollowStateTerminal | null | undefined): boolean {
  if (!term) return false;
  return !captureTerminalViewport(term).followOutput;
}
