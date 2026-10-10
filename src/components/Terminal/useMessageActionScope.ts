import { useSyncExternalStore } from "react";

const KEY = "ct-message-action-scope";
const listeners = new Set<() => void>();
let whole = false;
try { whole = sessionStorage.getItem(KEY) === "whole"; } catch {}
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
const snapshot = () => whole;

export function useMessageActionScope(): [boolean, (value: boolean) => void] {
  const value = useSyncExternalStore(subscribe, snapshot, snapshot);
  return [value, (next) => {
    whole = next;
    try { sessionStorage.setItem(KEY, next ? "whole" : "part"); } catch {}
    for (const listener of listeners) listener();
  }];
}
