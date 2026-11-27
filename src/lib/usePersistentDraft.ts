import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { editFrontendDraft, frontendSignals } from "./frontendUpdate/appState";
import { hasComposerDraft, readComposerDraft } from "./frontendUpdate/drafts";
import { composerDraftPersistence, type DraftPersistence } from "./frontendUpdate/draftPersistence";

export interface DraftSaveStatus {
  saveUnavailable: boolean;
}

function readDraft(scope: string, persistence: DraftPersistence): string {
  const drafts = frontendSignals.checkpointView().drafts;
  return hasComposerDraft(drafts, scope) ? readComposerDraft(drafts, scope) : persistence.records.get(scope) ?? "";
}

export const DraftPersistenceContext = createContext(composerDraftPersistence);

export function usePersistentDraft(
  scope: string,
  override?: DraftPersistence,
): [string, (value: string) => void, DraftSaveStatus] {
  const inherited = useContext(DraftPersistenceContext);
  const persistence = override ?? inherited;
  const [state, setState] = useState(() => ({
    scope,
    value: readDraft(scope, persistence),
    saveUnavailable: persistence.repeatedlyFailed(scope),
  }));
  const value = state.scope === scope ? state.value : readDraft(scope, persistence);
  const saveUnavailable = state.scope === scope ? state.saveUnavailable : persistence.repeatedlyFailed(scope);

  useEffect(() => {
    let active = true;
    const sync = () => {
      if (!active) return;
      const nextValue = readDraft(scope, persistence);
      const nextSaveUnavailable = persistence.repeatedlyFailed(scope);
      setState(previous =>
        previous.scope === scope && previous.value === nextValue && previous.saveUnavailable === nextSaveUnavailable
          ? previous
          : { scope, value: nextValue, saveUnavailable: nextSaveUnavailable },
      );
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
    setState(previous => ({
      scope,
      value: next,
      saveUnavailable: previous.scope === scope ? previous.saveUnavailable : persistence.repeatedlyFailed(scope),
    }));
  }, [scope, persistence]);
  return [value, edit, { saveUnavailable }];
}
