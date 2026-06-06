import { useEffect, useMemo, useRef, useState } from "react";

const PREFIX = "codeterm:composer-draft:";

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
    // Draft persistence is best-effort. Private mode/quota failures should
    // never block typing or sending.
  }
}

export function usePersistentDraft(scope: string): [string, (value: string) => void] {
  const key = useMemo(() => `${PREFIX}${scope}`, [scope]);
  const [value, setValue] = useState(() => readDraft(key));
  const skipSaveRef = useRef(false);

  useEffect(() => {
    skipSaveRef.current = true;
    setValue(readDraft(key));
  }, [key]);

  useEffect(() => {
    if (skipSaveRef.current) {
      skipSaveRef.current = false;
      return;
    }
    writeDraft(key, value);
  }, [key, value]);

  return [value, setValue];
}
