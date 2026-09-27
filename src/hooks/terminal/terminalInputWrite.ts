import { invoke } from "../../lib/bridge";
import { PtyInputWriteQueue } from "./ptyInputWriteQueue";

export const TERMINAL_INPUT_WRITE_FAILED_EVENT = "codeterm-terminal-input-write-failed";

const queue = new PtyInputWriteQueue(
  (ptyId, data, raw) =>
    invoke("write_pty_string", {
      ptyId,
      data,
      ...(raw === undefined ? {} : { raw }),
    }),
  (ptyId, dataLength, _error) => {
    console.error("[terminal-input] PTY write burst failed", { ptyId, dataLength });
    window.dispatchEvent(
      new CustomEvent(TERMINAL_INPUT_WRITE_FAILED_EVENT, { detail: { ptyId } }),
    );
  },
);

export function writeTerminalInput(ptyId: string, data: string, raw?: boolean): Promise<void> {
  return queue.enqueue(ptyId, data, raw);
}
