export interface HiddenOutputBuffer {
  push(chunk: Uint8Array): void;
  /** Concatenate and clear; `dropped` means the cap evicted older bytes, so a
   *  replay would be missing alt-screen/SGR state and the caller must resync. */
  drain(): { payload: Uint8Array; dropped: boolean };
  readonly size: number;
}

export function createHiddenOutputBuffer(maxBytes: number): HiddenOutputBuffer {
  let chunks: Uint8Array[] = [];
  let bytes = 0;
  let dropped = false;

  return {
    push(chunk) {
      if (chunk.length === 0) return;
      chunks.push(chunk);
      bytes += chunk.length;
      while (bytes > maxBytes && chunks.length > 1) {
        const old = chunks.shift()!;
        bytes -= old.length;
        dropped = true;
      }
    },
    drain() {
      const payload = new Uint8Array(bytes);
      let offset = 0;
      for (const chunk of chunks) {
        payload.set(chunk, offset);
        offset += chunk.length;
      }
      const wasDropped = dropped;
      chunks = [];
      bytes = 0;
      dropped = false;
      return { payload, dropped: wasDropped };
    },
    get size() {
      return bytes;
    },
  };
}
