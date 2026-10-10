import { invoke } from "../../lib/bridge";
import { relayRemoteTabAction } from "../../lib/remoteTabRoute";
import { classifyTabTarget, parseCompositeTabId } from "../../lib/remoteTabTransport";
import { PtyInputWriteQueue } from "./ptyInputWriteQueue";

export const TERMINAL_INPUT_WRITE_FAILED_EVENT = "codeterm-terminal-input-write-failed";

const queue = new PtyInputWriteQueue(
  (targetId, data, raw) => {
    const remote = parseCompositeTabId(targetId);
    if (remote) {
      return relayRemoteTabAction(
        { peerId: remote.peerId, tabId: remote.remoteTabId },
        "POST",
        "/pty/write",
        { data, raw: raw !== false },
      );
    }
    return invoke("write_pty_string", {
      ptyId: targetId,
      data,
      ...(raw === undefined ? {} : { raw }),
    });
  },
  (targetId, dataLength, _error) => {
    console.error("[terminal-input] PTY write burst failed", { targetId, dataLength });
    window.dispatchEvent(
      new CustomEvent(TERMINAL_INPUT_WRITE_FAILED_EVENT, { detail: { targetId } }),
    );
  },
);

export function terminalInputTarget(tabId: string, ptyId: string): string {
  const target = classifyTabTarget(tabId);
  if (target.kind === "invalid-remote") {
    throw new Error(`Invalid remote tab id "${tabId}"; local input fallback is forbidden`);
  }
  return target.kind === "remote" ? target.ordinaryTabId : ptyId;
}

export function writeTerminalInput(tabId: string, ptyId: string, data: string, raw?: boolean): Promise<void> {
  return queue.enqueue(terminalInputTarget(tabId, ptyId), data, raw);
}
