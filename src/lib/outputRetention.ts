import { planOutputReplay, type OutputBytesFrame } from "./outputReplay";

interface Retained extends Omit<OutputBytesFrame, "bytes"> { chunks: Uint8Array[]; size: number }
export class OutputRetention {
  private tabs = new Map<string, Retained>();
  constructor(private capacity = 2 * 1024 * 1024) {}
  clear() { this.tabs.clear(); }
  append(tabId: string, frame: OutputBytesFrame) {
    const old = this.tabs.get(tabId);
    const sameRing = old?.boot === frame.boot && old.ptyId === frame.ptyId;
    const retained = sameRing ? old : { boot: frame.boot, ptyId: frame.ptyId, offset: 0, chunks: [], size: 0 };
    const next = planOutputReplay(sameRing ? old : undefined, frame);
    if (next.bytes.length === 0) return;
    const start = frame.offset - next.bytes.length;
    if (sameRing && start > old.offset) { retained.chunks = []; retained.size = 0; }
    retained.chunks.push(next.bytes);
    retained.size += next.bytes.length;
    Object.assign(retained, next.cursor);
    while (retained.size > this.capacity) {
      const first = retained.chunks[0];
      const remove = Math.min(first.length, retained.size - this.capacity);
      retained.size -= remove;
      if (remove === first.length) retained.chunks.shift();
      else retained.chunks[0] = first.slice(remove);
    }
    this.tabs.set(tabId, retained);
  }
  snapshot(tabId: string): OutputBytesFrame | undefined {
    const retained = this.tabs.get(tabId);
    if (!retained) return;
    const bytes = new Uint8Array(retained.size);
    let index = 0;
    for (const chunk of retained.chunks) { bytes.set(chunk, index); index += chunk.length; }
    return { boot: retained.boot, ptyId: retained.ptyId, offset: retained.offset, bytes };
  }
}
