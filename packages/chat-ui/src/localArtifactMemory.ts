import type { LocalArtifactKind, LocalPreviewFile } from "./localArtifactLoader";

const MAX_ENTRIES = 32;
const MAX_CHARS = 4 * 1024 * 1024;

export interface LocalArtifactMemory {
  get(key: string): LocalPreviewFile | null;
  set(key: string, file: LocalPreviewFile): void;
  delete(key: string): void;
}

/** Keyed by owning tab as well, so a remembered file never reaches a scope the loader did not authorize. */
export function localArtifactMemoryKey(path: string, kind: LocalArtifactKind, tabId: string | undefined): string {
  return `${tabId ?? ""}\u0000${kind}\u0000${path}`;
}

/** Bounded least-recently-used store of loaded previews, by entry count and total characters. */
export function createLocalArtifactMemory(maxEntries = MAX_ENTRIES, maxChars = MAX_CHARS): LocalArtifactMemory {
  const entries = new Map<string, LocalPreviewFile>();
  let chars = 0;
  const remove = (key: string) => {
    const file = entries.get(key);
    if (!file) return;
    entries.delete(key);
    chars -= file.data.length;
  };
  return {
    get(key) {
      const file = entries.get(key);
      if (!file) return null;
      entries.delete(key);
      entries.set(key, file);
      return file;
    },
    set(key, file) {
      remove(key);
      if (file.data.length > maxChars) return;
      entries.set(key, file);
      chars += file.data.length;
      for (const oldest of entries.keys()) {
        if (entries.size <= maxEntries && chars <= maxChars) break;
        remove(oldest);
      }
    },
    delete: remove,
  };
}

// A transcript row remounts when it scrolls back into the mounted window; it must come back at its loaded height.
export const localArtifactMemory = createLocalArtifactMemory();
