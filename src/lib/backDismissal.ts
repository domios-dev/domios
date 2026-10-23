import { useEffect, useRef } from "react";

interface Entry {
  dismiss: () => void;
}

const stack: Entry[] = [];

/** Registers an open surface; system Back closes the newest registration first. */
export function pushBackDismissal(dismiss: () => void): () => void {
  const entry: Entry = { dismiss };
  stack.push(entry);
  return () => {
    const index = stack.indexOf(entry);
    if (index >= 0) stack.splice(index, 1);
  };
}

export function dismissTopSurface(): boolean {
  const entry = stack.pop();
  if (!entry) return false;
  entry.dismiss();
  return true;
}

export function useBackDismissal(open: boolean, dismiss: () => void): void {
  const latest = useRef(dismiss);
  latest.current = dismiss;
  useEffect(() => {
    if (!open) return;
    return pushBackDismissal(() => latest.current());
  }, [open]);
}
