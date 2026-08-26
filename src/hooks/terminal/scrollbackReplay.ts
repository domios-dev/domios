export type ScrollbackReplayOutcome = "replayed" | "empty" | "cancelled";

export interface ScrollbackReplayGate {
  loadTail: () => Promise<Uint8Array | null>;
  replay: (tail: Uint8Array, onComplete: () => void, onCancel: () => void) => void;
  isDisposed: () => boolean;
}

// Sidecar replay owns exactly the restart gap: a restored local tab with no
// live PTY yet. Live-buffer replay (ptyId set) and web/remote transports keep
// their existing ownership.
export function shouldReplayScrollbackSidecar(
  ptyId: string | null | undefined,
  isWeb: boolean,
  isRemote: boolean,
): boolean {
  return !ptyId && !isWeb && !isRemote;
}

/**
 * Resolves only once the sidecar tail settled into the display buffer, so the
 * caller attaches live output strictly after replay — never interleaved.
 * Missing, empty, or failing sidecars resolve "empty": blank tab, today's behavior.
 */
export async function replayScrollbackBeforeLiveAttach(
  gate: ScrollbackReplayGate,
): Promise<ScrollbackReplayOutcome> {
  let tail: Uint8Array | null = null;
  try {
    tail = await gate.loadTail();
  } catch {
    tail = null;
  }
  if (gate.isDisposed()) return "cancelled";
  if (!tail || tail.length === 0) return "empty";
  const bytes = tail;
  return new Promise((resolve) => {
    gate.replay(bytes, () => resolve("replayed"), () => resolve("cancelled"));
  });
}
