import type { Terminal } from "@xterm/xterm";

export const TERMINAL_WRITE_SLICE_BYTES = 64 * 1024;
// Drain multiple slices per frame while under this budget, then yield to the
// browser so keystrokes/paint stay responsive. Lifts the old one-slice-per-rAF
// 3.75MB/s throughput cap on replay/bursts without re-introducing UI freezes:
// once a frame's writes exceed the budget we hand control back via rAF.
export const TERMINAL_WRITE_FRAME_BUDGET_MS = 8;

export function writeTerminalBytesInFrames(
  term: Pick<Terminal, "write">,
  payload: Uint8Array,
  onComplete?: () => void,
): () => void {
  let offset = 0;
  let cancelled = false;
  let frame: number | null = null;

  const writeNext = () => {
    frame = null;
    if (cancelled) return;
    if (offset >= payload.length) {
      onComplete?.();
      return;
    }
    const frameStart = performance.now();
    const step = () => {
      if (cancelled) return;
      if (offset >= payload.length) {
        onComplete?.();
        return;
      }
      const nextOffset = Math.min(offset + TERMINAL_WRITE_SLICE_BYTES, payload.length);
      const chunk = payload.slice(offset, nextOffset);
      offset = nextOffset;
      term.write(chunk, () => {
        if (cancelled) return;
        if (offset >= payload.length) {
          onComplete?.();
          return;
        }
        if (performance.now() - frameStart < TERMINAL_WRITE_FRAME_BUDGET_MS) step();
        else frame = requestAnimationFrame(writeNext);
      });
    };
    step();
  };

  frame = requestAnimationFrame(writeNext);
  return () => {
    cancelled = true;
    if (frame != null) cancelAnimationFrame(frame);
  };
}

export function writeTerminalStringInFrames(
  term: Pick<Terminal, "write">,
  payload: string,
  onComplete?: () => void,
): () => void {
  let offset = 0;
  let cancelled = false;
  let frame: number | null = null;

  const writeNext = () => {
    frame = null;
    if (cancelled) return;
    if (offset >= payload.length) {
      onComplete?.();
      return;
    }
    const frameStart = performance.now();
    const step = () => {
      if (cancelled) return;
      if (offset >= payload.length) {
        onComplete?.();
        return;
      }
      const nextOffset = Math.min(offset + TERMINAL_WRITE_SLICE_BYTES, payload.length);
      const chunk = payload.slice(offset, nextOffset);
      offset = nextOffset;
      term.write(chunk, () => {
        if (cancelled) return;
        if (offset >= payload.length) {
          onComplete?.();
          return;
        }
        if (performance.now() - frameStart < TERMINAL_WRITE_FRAME_BUDGET_MS) step();
        else frame = requestAnimationFrame(writeNext);
      });
    };
    step();
  };

  frame = requestAnimationFrame(writeNext);
  return () => {
    cancelled = true;
    if (frame != null) cancelAnimationFrame(frame);
  };
}
