import type { Terminal } from "@xterm/xterm";

type HistoryPayload = Uint8Array | string;

export function writeTerminalHistory(
  term: Pick<Terminal, "write">,
  payload: HistoryPayload,
  onComplete?: () => void,
): () => void {
  let cancelled = false;
  term.write(payload, () => {
    if (cancelled) return;
    term.write("", () => {
      if (!cancelled) onComplete?.();
    });
  });
  return () => {
    cancelled = true;
  };
}
