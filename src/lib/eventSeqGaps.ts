/**
 * Contiguity check for the system-event stream.
 *
 * `/api/events/stream` promises that loss is signalled by exactly one
 * `overflow` frame and never by a silent gap. That promise does not hold
 * today: seq allocation and the bus publish are not atomic, so concurrent
 * publishers can reach the broadcast out of order and the stream's monotonic
 * guard drops the lower seq without a word (rollacode/codeterm#428 family).
 *
 * This is the client noticing. A gap is treated as an overflow so the mirror
 * self-heals, and counted so the daemon defect stays visible in the console
 * and in bench rows instead of disappearing behind the recovery.
 */
export type SeqVerdict = "ok" | "gap" | "duplicate" | "origin-changed";

export interface SeqObservation {
  verdict: SeqVerdict;
  /** The seq this frame should have carried, or null when there is no baseline. */
  expected: number | null;
}

export interface SeqGapTracker {
  observe(boot: string, seq: number): SeqObservation;
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
    observe(nextBoot, seq) {
      if (boot !== nextBoot || last === null) {
        boot = nextBoot;
        last = seq;
        return { verdict: "origin-changed", expected: null };
      }
      if (seq <= last) return { verdict: "duplicate", expected: last + 1 };
      const expected = last + 1;
      last = seq;
      if (seq === expected) return { verdict: "ok", expected };
      gaps += 1;
      return { verdict: "gap", expected };
    },
    adopt(nextBoot, seq) {
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
