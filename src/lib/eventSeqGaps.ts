export type SeqVerdict = "ok" | "gap" | "duplicate" | "origin-changed";

export interface SeqObservation {
  verdict: SeqVerdict;
  /** The seq this frame should have carried, or null when there is no baseline. */
  expected: number | null;
}

export interface SeqGapTracker {
  observe(boot: string, seq: number, previous?: number | null): SeqObservation;
  /** Set the baseline without judging it: after an overflow, or a cursor jump. */
  adopt(boot: string, seq: number): void;
  gaps(): number;
  reset(): void;
}

export function createSeqGapTracker(): SeqGapTracker {
  let boot: string | null = null;
  let last: number | null = null;
  let gaps = 0;

  return {
    observe(nextBoot, seq, previous) {
      if (boot !== nextBoot || last === null || !Number.isSafeInteger(last)) {
        boot = nextBoot;
        last = seq;
        return { verdict: "origin-changed", expected: null };
      }
      if (seq <= last) return { verdict: "duplicate", expected: last + 1 };
      const expected = last + 1;
      const lastBefore = last;
      last = seq;
      if (previous === lastBefore || (previous === undefined && seq === expected)) return { verdict: "ok", expected };
      gaps += 1;
      return { verdict: "gap", expected };
    },
    adopt(nextBoot, seq) {
      // A malformed baseline would make every later frame look like a gap.
      if (!Number.isSafeInteger(seq)) return;
      boot = nextBoot;
      last = seq;
    },
    gaps: () => gaps,
    reset() {
      boot = null;
      last = null;
    },
  };
}
