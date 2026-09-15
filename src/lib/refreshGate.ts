/** Turns change notices into re-reads: at most one per animation frame, plus one trailing read per in-flight read. */
export interface RefreshGate {
  request(): void;
  /** The reader calls this when its newest read lands, whoever started it. */
  settled(): void;
  cancel(): void;
}

export function createRefreshGate(run: () => void): RefreshGate {
  let frame: number | null = null;
  let inFlight = false;
  let queued = false;

  const fire = () => {
    frame = null;
    inFlight = true;
    run();
  };
  const schedule = () => {
    if (frame === null) frame = requestAnimationFrame(fire);
  };

  return {
    request() {
      if (inFlight) queued = true;
      else schedule();
    },
    settled() {
      inFlight = false;
      if (!queued) return;
      queued = false;
      schedule();
    },
    cancel() {
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      queued = false;
    },
  };
}
