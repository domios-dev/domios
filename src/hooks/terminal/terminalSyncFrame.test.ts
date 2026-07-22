import { SyncFrameCoalescer } from "./terminalSyncFrame";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

const enc = (s: string) => new TextEncoder().encode(s);
const dec = (u: Uint8Array) => new TextDecoder().decode(u);
const BEGIN = "\x1b[?2026h";
const END = "\x1b[?2026l";

// Emitting all chunks then concatenating must reproduce the input byte-for-byte.
function roundtrip(pieces: string[]): { out: string; emitCount: number } {
  const g = new SyncFrameCoalescer();
  const emits: Uint8Array[] = [];
  for (const p of pieces) emits.push(...g.push(enc(p)));
  const tail = g.flush();
  if (tail) emits.push(tail);
  const total = emits.reduce((n, e) => n + e.length, 0);
  const merged = new Uint8Array(total);
  let o = 0;
  for (const e of emits) { merged.set(e, o); o += e.length; }
  return { out: dec(merged), emitCount: emits.length };
}

// 1. Plain output passes straight through, one emit per non-empty push.
{
  const g = new SyncFrameCoalescer();
  const e = g.push(enc("hello world"));
  assert(e.length === 1 && dec(e[0]) === "hello world", "plain output passes through unchanged");
  assert(!g.holding, "no hold for plain output");
}

// 2. A whole frame in one chunk emits pre-text separately, then the frame as one chunk.
{
  const g = new SyncFrameCoalescer();
  const e = g.push(enc(`abc${BEGIN}line1\r\nline2${END}xyz`));
  assert(dec(e[0]) === "abc", "pre-frame text flushed immediately");
  assert(dec(e[1]) === `${BEGIN}line1\r\nline2${END}`, "frame coalesced with its markers");
  assert(dec(e[2]) === "xyz", "post-frame text flushed");
}

// 3. A frame split across many chunks is held until close, then emitted as ONE chunk.
{
  const g = new SyncFrameCoalescer();
  let emits: Uint8Array[] = [];
  emits.push(...g.push(enc(BEGIN)));
  emits.push(...g.push(enc("aaa")));
  emits.push(...g.push(enc("bbb")));
  assert((emits as unknown[]).length === 0, "nothing emitted while frame is open");
  assert(g.holding, "coalescer reports holding an open frame");
  emits.push(...g.push(enc(`ccc${END}`)));
  assert((emits as unknown[]).length === 1, "the whole frame emits as a single write");
  assert(dec(emits[0]) === `${BEGIN}aaabbbccc${END}`, "held frame body is intact and in order");
}

// 4. BEGIN marker split across a chunk boundary is detected.
{
  const g = new SyncFrameCoalescer();
  const emits: Uint8Array[] = [];
  emits.push(...g.push(enc("text\x1b[?20"))); // partial BEGIN at tail
  emits.push(...g.push(enc("26hFRAME" + END))); // completes BEGIN
  const merged = emits.map(dec).join("");
  assert(merged === `text${BEGIN}FRAME${END}`, "split BEGIN reassembles losslessly");
  // "text" must have been emitted before the frame content
  assert(dec(emits[0]) === "text", "pre-frame text emitted despite split marker");
}

// 5. END marker split across a chunk boundary is detected.
{
  const g = new SyncFrameCoalescer();
  const emits: Uint8Array[] = [];
  emits.push(...g.push(enc(`${BEGIN}body\x1b[?2026`))); // partial END
  assert((emits as unknown[]).length === 0, "frame stays open on partial END");
  emits.push(...g.push(enc("l"))); // completes END
  assert((emits as unknown[]).length === 1 && dec(emits[0]) === `${BEGIN}body${END}`, "split END closes the frame");
}

// 6. Two frames plus interleaved plain text in one chunk.
{
  const rt = roundtrip([`${BEGIN}f1${END}mid${BEGIN}f2${END}end`]);
  assert(rt.out === `${BEGIN}f1${END}mid${BEGIN}f2${END}end`, "two frames + text round-trip");
}

// 7. Fuzz: arbitrary splitting of a fixed stream is always lossless.
{
  const stream = `start${BEGIN}alpha\r\nbeta${END}between${BEGIN}gamma${END}done`;
  const bytes = enc(stream);
  for (const step of [1, 2, 3, 5, 7, 8, 13]) {
    const pieces: string[] = [];
    for (let i = 0; i < bytes.length; i += step) {
      pieces.push(dec(bytes.subarray(i, i + step)));
    }
    // dec/enc on arbitrary byte splits is safe here: the stream is ASCII.
    const rt = roundtrip(pieces);
    assert(rt.out === stream, `lossless at split step ${step}`);
  }
}

// 8. Nested BEGIN inside an open frame does not prematurely close; first END closes.
{
  const g = new SyncFrameCoalescer();
  const emits: Uint8Array[] = [];
  emits.push(...g.push(enc(`${BEGIN}a${BEGIN}b${END}tail`)));
  assert(dec(emits[0]) === `${BEGIN}a${BEGIN}b${END}`, "first END closes; inner BEGIN is body");
  assert(dec(emits[1]) === "tail", "post-frame tail emitted");
}

// 9. flush() drains a never-closed frame (teardown backstop).
{
  const g = new SyncFrameCoalescer();
  const e = g.push(enc(`${BEGIN}unterminated`));
  assert((e as unknown[]).length === 0, "open frame withheld");
  const tail = g.flush();
  assert(tail !== null && dec(tail) === `${BEGIN}unterminated`, "flush drains held bytes");
  assert(!g.holding, "flush clears state");
}

console.log("terminal sync-frame coalescer checks passed");
