import { useCallback, useEffect, useState } from "react";
import { editFrontendDraft, frontendSignals } from "./frontendUpdate/appState";
import { hasComposerDraft, readComposerDraft, writeLocalDraft } from "./frontendUpdate/drafts";

function readDraft(scope: string): string {
  return readComposerDraft(frontendSignals.checkpoint().drafts, window.localStorage, scope);
}

export function usePersistentDraft(scope: string): [string, (value: string) => void] {
  const [draft, setDraft] = useState(() => ({ scope, value: readDraft(scope) }));
  const value = draft.scope === scope ? draft.value : readDraft(scope);

  useEffect(() => {
    const restored = readDraft(scope);
    if (!hasComposerDraft(frontendSignals.checkpoint().drafts, scope)) editFrontendDraft(scope, restored);
    setDraft({ scope, value: restored });
  }, [scope]);

  const edit = useCallback((next: string) => {
    writeLocalDraft(window.localStorage, scope, next);
    editFrontendDraft(scope, next);
    setDraft({ scope, value: next });
  }, [scope]);
  return [value, edit];
}
