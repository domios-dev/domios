import type { Terminal } from "@xterm/xterm";

export const TERMINAL_WRITE_SLICE_BYTES = 64 * 1024;

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
    const nextOffset = Math.min(offset + TERMINAL_WRITE_SLICE_BYTES, payload.length);
    const chunk = payload.slice(offset, nextOffset);
    offset = nextOffset;
    term.write(chunk, () => {
      if (cancelled) return;
      if (offset >= payload.length) onComplete?.();
      else frame = requestAnimationFrame(writeNext);
    });
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
    const nextOffset = Math.min(offset + TERMINAL_WRITE_SLICE_BYTES, payload.length);
    const chunk = payload.slice(offset, nextOffset);
    offset = nextOffset;
    term.write(chunk, () => {
      if (cancelled) return;
      if (offset >= payload.length) onComplete?.();
      else frame = requestAnimationFrame(writeNext);
    });
  };

  frame = requestAnimationFrame(writeNext);
  return () => {
    cancelled = true;
    if (frame != null) cancelAnimationFrame(frame);
  };
}
