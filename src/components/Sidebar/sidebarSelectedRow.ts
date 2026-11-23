import { createContext, useCallback, useContext, type RefObject } from "react";

export interface SidebarSelectedRow {
  tabId: string;
  title: string;
  element: HTMLElement;
}

export const SidebarSelectedRowContext = createContext<((row: SidebarSelectedRow) => () => void) | null>(null);

export function useSidebarSelectedRow<T extends HTMLElement>(
  tabId: string, title: string, selected: boolean, rowRef?: RefObject<T | null>,
) {
  const register = useContext(SidebarSelectedRowContext);
  const selectedRef = useCallback((element: T | null) => {
    if (!element || !register) return;
    if (rowRef) rowRef.current = element;
    const unregister = register({ tabId, title, element });
    return () => {
      if (rowRef) rowRef.current = null;
      unregister();
    };
  }, [register, rowRef, tabId, title]);
  return selected && register ? selectedRef : rowRef;
}
