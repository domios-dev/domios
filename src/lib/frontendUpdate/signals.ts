import type { ClientCheckpoint } from "./checkpoint";
import type { Safety } from "./model";
import type { FrontendSignals } from "./runtime";

export function createFrontendSignals(initial: ClientCheckpoint, safety: Safety, restore: (checkpoint: ClientCheckpoint) => ClientCheckpoint | void): FrontendSignals & {
  edit(checkpoint: ClientCheckpoint): void;
  signal(patch: Partial<Safety>): void;
} {
  let checkpoint = structuredClone(initial);
  let currentSafety = { ...safety };
  const listeners = new Set<(change: "checkpoint" | "safety") => void>();
  const emit = (change: "checkpoint" | "safety") => { for (const listener of listeners) listener(change); };
  return {
    checkpoint: () => structuredClone(checkpoint),
    safety: () => ({ ...currentSafety }),
    hydrate(value) { checkpoint = structuredClone(restore(value) ?? value); },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    edit(value) { checkpoint = structuredClone(value); emit("checkpoint"); },
    signal(patch) { currentSafety = { ...currentSafety, ...patch }; emit("safety"); },
  };
}
