/** The app's real speeds (spec L11.2): a person types, an agent streams in bursts, a tool takes measurable time. */

/** Deterministic PRNG, so every visit to a place plays the same script. */
export function seeded(seed: number): () => number {
  let s = (seed >>> 0) || 0x9e3779b9;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 0x1_0000_0000;
  };
}

const between = (r: () => number, lo: number, hi: number) => lo + (hi - lo) * r();

export const TYPE_CHAR = [0.055, 0.075] as const;
export const TYPE_PUNCT_PAUSE = [0.25, 0.4] as const;
export const TYPE_THINK_PAUSE = [0.6, 0.9] as const;
export const STREAM_BURST = [3, 12] as const;
export const STREAM_GAP = [0.03, 0.06] as const;
export const TOOL_SPAN = [0.6, 2.5] as const;

/** Seconds after the start at which each character of `text` appears as a person types it. */
export function typingTimes(text: string, seed: number): number[] {
  const r = seeded(seed);
  const times: number[] = [];
  let t = 0;
  for (let i = 0; i < text.length; i++) {
    t += between(r, TYPE_CHAR[0], TYPE_CHAR[1]);
    times.push(t);
    const ch = text[i];
    if (/[.,;:?!]/.test(ch) && i < text.length - 1) t += between(r, TYPE_PUNCT_PAUSE[0], TYPE_PUNCT_PAUSE[1]);
    else if (ch === " " && r() < 0.04) t += between(r, TYPE_THINK_PAUSE[0], TYPE_THINK_PAUSE[1]);
  }
  return times;
}

export interface Burst {
  at: number;
  end: number;
}

/** Token bursts of an agent reply: 3-12 characters every 30-60 ms, never letter by letter. */
export function streamBursts(text: string, seed: number): Burst[] {
  const r = seeded(seed);
  const bursts: Burst[] = [];
  let end = 0;
  let at = 0;
  while (end < text.length) {
    end = Math.min(text.length, end + Math.round(between(r, STREAM_BURST[0], STREAM_BURST[1])));
    bursts.push({ at, end });
    at += between(r, STREAM_GAP[0], STREAM_GAP[1]);
  }
  return bursts;
}

/** How many characters are visible `t` seconds after the start. */
export function shownChars(times: number[], t: number): number {
  let lo = 0;
  let hi = times.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] <= t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function streamedChars(bursts: Burst[], t: number): number {
  let shown = 0;
  for (const burst of bursts) {
    if (burst.at > t) break;
    shown = burst.end;
  }
  return shown;
}

/** A tool's running time, clamped to the range a real card shows. */
export function toolSpan(seconds: number): number {
  return Math.min(TOOL_SPAN[1], Math.max(TOOL_SPAN[0], seconds));
}
