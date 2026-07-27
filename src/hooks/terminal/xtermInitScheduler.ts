// Staggers xterm initialization across animation frames so N tabs mounting at
// once (parallel agent spawns) don't stack their heavy init into one long frame.

export type FrameScheduler = (cb: () => void) => void;

const defaultFrame: FrameScheduler = (cb) => {
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(() => cb());
  else setTimeout(cb, 0);
};

interface Entry {
  task: () => void;
  priority: boolean;
  cancelled: boolean;
}

let queue: Entry[] = [];
let draining = false;
let frameImpl: FrameScheduler = defaultFrame;

// Test seam: drive the queue with a deterministic clock instead of rAF.
export function setXtermFrameScheduler(fn: FrameScheduler | null): void {
  frameImpl = fn ?? defaultFrame;
}

function kick(): void {
  if (draining) return;
  draining = true;
  frameImpl(drain);
}

function drain(): void {
  draining = false;
  let idx = queue.findIndex((e) => !e.cancelled && e.priority);
  if (idx < 0) idx = queue.findIndex((e) => !e.cancelled);
  if (idx < 0) {
    queue = [];
    return;
  }
  const entry = queue.splice(idx, 1)[0];
  try {
    entry.task();
  } finally {
    if (queue.some((e) => !e.cancelled)) kick();
    else queue = [];
  }
}

// Returns a cancel handle; cancelling before the task runs skips it.
export function scheduleXtermInit(task: () => void, opts?: { priority?: boolean }): () => void {
  const entry: Entry = { task, priority: opts?.priority ?? false, cancelled: false };
  queue.push(entry);
  kick();
  return () => {
    entry.cancelled = true;
  };
}
