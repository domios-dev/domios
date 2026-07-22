// DEC 2026 synchronized-output frames (`CSI ? 2026 h` … `CSI ? 2026 l`) tell the
// terminal to present the enclosed update atomically. xterm buffers rows during a
// frame but force-flushes after a hard-coded 1s safety timeout, so a provider whose
// frame arrives across many chunks over >1s (a full-screen TUI rebuilding its
// transcript on resize) renders progressively — visible scroll/churn. Coalescing
// each frame into a single write lets xterm reach the close marker inside one parse,
// rendering the frame once. Capability-based, provider-neutral.

type Bytes = Uint8Array<ArrayBufferLike>;

const SYNC_BEGIN = Uint8Array.of(0x1b, 0x5b, 0x3f, 0x32, 0x30, 0x32, 0x36, 0x68); // ESC [ ? 2 0 2 6 h
const SYNC_END = Uint8Array.of(0x1b, 0x5b, 0x3f, 0x32, 0x30, 0x32, 0x36, 0x6c); // ESC [ ? 2 0 2 6 l
const MARKER_LEN = SYNC_BEGIN.length;

// A frame that never closes must not stall output forever; flush once it grows past
// this. Kept below the read loop's 128 KiB pause watermark so an oversized/dead
// frame degrades to passthrough instead of deadlocking the reader. Real frames are
// a few KB, so this only fires on a malformed/runaway stream.
const MAX_HELD_BYTES = 96 * 1024;

function indexOf(hay: Bytes, needle: Bytes, from: number): number {
  const last = hay.length - needle.length;
  outer: for (let i = from; i <= last; i++) {
    for (let j = 0; j < needle.length; j++) {
      if (hay[i + j] !== needle[j]) continue outer;
    }
    return i;
  }
  return -1;
}

// Longest suffix of `buf` that is a proper prefix of `needle` (a marker possibly
// split across the chunk boundary). Bounded by MARKER_LEN - 1.
function partialTail(buf: Bytes, needle: Bytes): number {
  const max = Math.min(needle.length - 1, buf.length);
  for (let len = max; len > 0; len--) {
    let match = true;
    for (let j = 0; j < len; j++) {
      if (buf[buf.length - len + j] !== needle[j]) { match = false; break; }
    }
    if (match) return len;
  }
  return 0;
}

function concat(a: Bytes, b: Bytes): Bytes {
  if (a.length === 0) return b;
  if (b.length === 0) return a;
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

// Stateful across chunks: non-sync bytes pass through immediately; bytes inside a
// synchronized-output frame are held and emitted as one chunk when the frame closes.
export class SyncFrameCoalescer {
  private inFrame = false;
  private acc: Bytes = new Uint8Array(0); // unresolved bytes (held frame body, or a split-marker tail)

  push(chunk: Bytes): Bytes[] {
    const emits: Bytes[] = [];
    let work = concat(this.acc, chunk);
    this.acc = new Uint8Array(0);
    let cursor = 0;

    for (;;) {
      if (!this.inFrame) {
        const begin = indexOf(work, SYNC_BEGIN, cursor);
        if (begin === -1) {
          const hold = partialTail(work.subarray(cursor), SYNC_BEGIN);
          const end = work.length - hold;
          if (end > cursor) emits.push(work.slice(cursor, end));
          this.acc = work.slice(end);
          break;
        }
        if (begin > cursor) emits.push(work.slice(cursor, begin));
        this.inFrame = true;
        // Rebase so `acc`/held always starts at the frame's opening marker.
        work = work.slice(begin);
        cursor = MARKER_LEN;
      } else {
        const end = indexOf(work, SYNC_END, cursor);
        if (end === -1) {
          if (work.length >= MAX_HELD_BYTES) {
            emits.push(work);
            this.inFrame = false;
            this.acc = new Uint8Array(0);
            break;
          }
          this.acc = work; // keep the whole open frame, re-scan its tail next push
          break;
        }
        emits.push(work.slice(0, end + MARKER_LEN));
        this.inFrame = false;
        work = work.slice(end + MARKER_LEN);
        cursor = 0;
        if (work.length === 0) break;
      }
    }
    return emits;
  }

  // Emit whatever is held (teardown, or a safety backstop for a frame that never
  // closes). Resets frame state.
  flush(): Bytes | null {
    if (this.acc.length === 0) { this.inFrame = false; return null; }
    const held = this.acc;
    this.acc = new Uint8Array(0);
    this.inFrame = false;
    return held;
  }

  get holding(): boolean {
    return this.acc.length > 0;
  }

  get heldBytes(): number {
    return this.acc.length;
  }
}
