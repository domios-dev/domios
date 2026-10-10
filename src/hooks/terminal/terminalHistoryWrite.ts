import type { Terminal } from "@xterm/xterm";
import { writeTerminalOutput } from "./terminalOutputWrite";

type HistoryPayload = Uint8Array | string;

export function writeTerminalHistory(
  term: Pick<Terminal, "write"> & Partial<Pick<Terminal, "options">>,
  payload: HistoryPayload,
  onComplete?: () => void,
): () => void {
  let cancelled = false;
  writeTerminalOutput(term, payload, () => {
    if (cancelled) return;
    term.write("", () => {
      if (!cancelled) onComplete?.();
    });
  });
  return () => {
    cancelled = true;
  };
}
