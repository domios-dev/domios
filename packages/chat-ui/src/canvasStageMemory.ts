import type { ViewportTransform } from "@codeterm/canvas-camera";

export interface CanvasStageMemory { transform: ViewportTransform | null; userMoved: boolean }
const memories = new Map<string, CanvasStageMemory>();

export function createCanvasStageMemory(key?: string, external?: CanvasStageMemory, store = memories) {
  const memory = external ?? (key === undefined ? null : store.get(key) ?? { transform: null, userMoved: false });
  return {
    read: () => memory,
    remember(transform: ViewportTransform, userMoved: boolean) {
      if (!memory) return;
      memory.transform = { ...transform };
      memory.userMoved = userMoved;
      if (key === undefined) return;
      store.delete(key);
      store.set(key, memory);
      if (store.size > 24) {
        const oldest = store.keys().next().value;
        if (oldest !== undefined) store.delete(oldest);
      }
    },
  };
}
