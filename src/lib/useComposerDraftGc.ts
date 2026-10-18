import { useEffect, useRef } from "react";
import { createComposerDraftPruner } from "./composerDraftGc";

export function useComposerDraftGc(tabs: Readonly<Record<string, unknown>>): void {
  const prune = useRef<ReturnType<typeof createComposerDraftPruner> | null>(null);
  useEffect(() => {
    try {
      prune.current ??= createComposerDraftPruner(window.localStorage);
      prune.current(new Set(Object.keys(tabs)));
    } catch {}
  }, [tabs]);
}
