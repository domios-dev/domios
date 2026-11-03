import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { editFrontendDraft, frontendSignals } from "./frontendUpdate/appState";
import { hasComposerDraft, readComposerDraft } from "./frontendUpdate/drafts";
import { composerDraftPersistence, type DraftPersistence } from "./frontendUpdate/draftPersistence";

function readDraft(scope: string, persistence: DraftPersistence): string {
  const drafts = frontendSignals.checkpointView().drafts;
  return hasComposerDraft(drafts, scope) ? readComposerDraft(drafts, scope) : persistence.records.get(scope) ?? "";
}

export const DraftPersistenceContext = createContext(composerDraftPersistence);

export function usePersistentDraft(scope: string, onError?: (message: string) => void,
  override?: DraftPersistence): [string, (value: string) => void] {
  const inherited = useContext(DraftPersistenceContext);
  const persistence = override ?? inherited;
  const [draft, setDraft] = useState(() => ({ scope, value: readDraft(scope, persistence) }));
  const errorCallback = useRef(onError);
  errorCallback.current = onError;
  const value = draft.scope === scope ? draft.value : readDraft(scope, persistence);

  useEffect(() => {
    let active = true;
    let reported: Error | undefined;
    const sync = () => {
      if (!active) return;
      const error = persistence.error(scope);
      const changedError = error && error !== reported;
      reported = error;
      if (changedError) errorCallback.current?.(`Draft could not be saved: ${error.message}`);
      const value = readDraft(scope, persistence);
      setDraft(previous => previous.scope === scope && previous.value === value ? previous : { scope, value });
    };
    const stop = persistence.subscribe(sync);
    void persistence.initialize().then(() => {
      if (!active) return;
      const restored = readDraft(scope, persistence);
      if (!hasComposerDraft(frontendSignals.checkpointView().drafts, scope)) editFrontendDraft(scope, restored);
      sync();
    });
    sync();
    const flush = () => { void persistence.flush().catch(() => {}); };
    window.addEventListener("blur", flush);
    window.addEventListener("pagehide", flush);
    return () => {
      active = false;
      stop();
      flush();
      window.removeEventListener("blur", flush);
      window.removeEventListener("pagehide", flush);
    };
  }, [scope, persistence]);

  const edit = useCallback((next: string) => {
    editFrontendDraft(scope, next);
    persistence.edit(scope, next);
    setDraft({ scope, value: next });
  }, [scope, persistence]);
  return [value, edit];
}
