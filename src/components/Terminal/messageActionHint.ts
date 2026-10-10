export const MESSAGE_HINT_KEY = "ct.message-actions-hints";

interface HintCount {
  hints: number;
  opens: number;
}

type HintStorage = Pick<Storage, "getItem" | "setItem">;

export function createHintBudget(storage: () => HintStorage) {
  let memory: HintCount = { hints: 0, opens: 0 };
  const count = (value: unknown) => typeof value === "number" && Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
  const read = () => {
    try {
      const saved = JSON.parse(storage().getItem(MESSAGE_HINT_KEY) ?? "null") as Partial<HintCount> | null;
      memory = { hints: Math.max(memory.hints, count(saved?.hints)), opens: Math.max(memory.opens, count(saved?.opens)) };
    } catch {}
    return memory;
  };
  const write = () => {
    try { storage().setItem(MESSAGE_HINT_KEY, JSON.stringify(memory)); } catch {}
  };
  return {
    claim() {
      const current = read();
      if (current.hints >= 10 || current.opens >= 2) return false;
      memory.hints++;
      write();
      return true;
    },
    opened() {
      read();
      memory.opens = Math.min(memory.opens + 1, 2);
      write();
    },
  };
}

export const messageHintBudget = createHintBudget(() => localStorage);

export function createStillHoverHint({ schedule, cancel, allowed, claim, show }: {
  schedule: (callback: () => void, delay: number) => unknown;
  cancel: (handle: unknown) => void;
  allowed: () => boolean;
  claim: () => boolean;
  show: () => void;
}) {
  let pending: unknown;
  let hovered = false;
  let used = false;
  const stop = () => { if (pending !== undefined) cancel(pending); pending = undefined; };
  const move = () => {
    stop();
    if (!hovered || used || !allowed()) return;
    pending = schedule(() => {
      pending = undefined;
      if (!hovered || used || !allowed()) return;
      used = true;
      if (claim()) show();
    }, 600);
  };
  return {
    enter() { hovered = true; used = false; move(); },
    move,
    leave() { hovered = false; stop(); },
    stop() { used = true; stop(); },
  };
}
