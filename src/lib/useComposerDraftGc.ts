import { useEffect } from "react";
import { composerDraftPersistence } from "./frontendUpdate/draftPersistence";

export function useComposerDraftGc(tabs: Readonly<Record<string, unknown>>): void {
  useEffect(() => {
    void composerDraftPersistence.prune(new Set(Object.keys(tabs)));
  }, [tabs]);
}
