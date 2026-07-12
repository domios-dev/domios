import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { COMPOSER_DRAFT_PREFIX } from "./composerDraftGc";

const PREFIX = COMPOSER_DRAFT_PREFIX;

function readDraft(key: string): string {
  try {
    return window.localStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

function writeDraft(key: string, value: string) {
  try {
    if (value) {
      window.localStorage.setItem(key, value);
    } else {
      window.localStorage.removeItem(key);
    }
  } catch {
    // Draft persistence is best-effort. Private mode/quota failures should never block typing or sending.
  }
}

const SAVE_DELAY_MS = 350;

export function usePersistentDraft(scope: string): [string, (value: string) => void] {
  const key = useMemo(() => `${PREFIX}${scope}`, [scope]);
  const [value, setValue] = useState(() => readDraft(key));
  const skipSaveRef = useRef(false);
  const valueRef = useRef(value);
  const saveTimerRef = useRef<number | null>(null);

  const clearSaveTimer = useCallback(() => {
    if (saveTimerRef.current == null) return;
    window.clearTimeout(saveTimerRef.current);
    saveTimerRef.current = null;
  }, []);

  const flushDraft = useCallback(() => {
    clearSaveTimer();
    writeDraft(key, valueRef.current);
  }, [clearSaveTimer, key]);

  useEffect(() => {
    skipSaveRef.current = true;
    setValue(readDraft(key));
    valueRef.current = readDraft(key);
  }, [key]);

  useEffect(() => {
    valueRef.current = value;
    if (skipSaveRef.current) {
      skipSaveRef.current = false;
      return;
    }
    clearSaveTimer();
    saveTimerRef.current = window.setTimeout(flushDraft, SAVE_DELAY_MS);
    return clearSaveTimer;
  }, [clearSaveTimer, flushDraft, key, value]);

  useEffect(() => {
    window.addEventListener("pagehide", flushDraft);
    window.addEventListener("blur", flushDraft);
    return () => {
      window.removeEventListener("pagehide", flushDraft);
      window.removeEventListener("blur", flushDraft);
      flushDraft();
    };
  }, [flushDraft]);

  return [value, setValue];
}
