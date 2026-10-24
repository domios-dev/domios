import { useCallback, useEffect, useState, type SetStateAction } from "react";
import { apiHost, relayTarget } from "../lib/webApi";
import { readNavigation, writeNavigation, type NavigationDecoder } from "../lib/viewerNavigation";

function viewerStorage(): Storage | null {
  try { return window.sessionStorage; } catch { return null; }
}

function viewerScope(): string {
  try { return `domios.navigation:${apiHost()}:${relayTarget()?.id ?? "self"}`; }
  catch { return "domios.navigation:self"; }
}

export function useViewerNavigation<T>(key: string, initial: T | (() => T), decode: NavigationDecoder<T>, restore = true) {
  const scope = viewerScope();
  const load = () => ({ scope, key, value: (restore ? readNavigation(viewerStorage(), scope, key, decode) : undefined)
    ?? (typeof initial === "function" ? (initial as () => T)() : initial) });
  const [stored, setStored] = useState(load);
  const current = stored.scope === scope && stored.key === key ? stored : load();
  if (current !== stored) setStored(current);
  const setValue = useCallback((next: SetStateAction<T>) => setStored((previous) => {
    const value = typeof next === "function" ? (next as (value: T) => T)(previous.value) : next;
    return Object.is(value, previous.value) ? previous : { ...previous, value };
  }), []);
  useEffect(() => { writeNavigation(viewerStorage(), current.scope, current.key, current.value, decode); }, [current, decode]);
  return [current.value, setValue] as const;
}
