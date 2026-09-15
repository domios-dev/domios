import { useSyncExternalStore } from "react";
import { SHELL_NARROW_QUERY } from "../../../../src/lib/shellBreakpoint";

function media(): MediaQueryList | null {
  return typeof window.matchMedia === "function" ? window.matchMedia(SHELL_NARROW_QUERY) : null;
}

/** The app shell's own phone boundary, read against the view's width. */
export function useNarrow(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const query = media();
      query?.addEventListener("change", onChange);
      return () => query?.removeEventListener("change", onChange);
    },
    () => media()?.matches ?? false,
    () => false,
  );
}
