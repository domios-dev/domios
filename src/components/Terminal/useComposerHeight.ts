import { useLayoutEffect, type RefObject } from "react";

export function useComposerHeight(ref: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const shell = ref.current;
    const root = shell?.closest<HTMLElement>('[data-testid="terminal-tab"]');
    if (!shell || !root) return;
    const surface = shell.closest<HTMLElement>("[data-composer-height-surface]") ?? shell;
    const publish = () => root.style.setProperty("--ct-composer-height", `${Math.ceil(surface.getBoundingClientRect().height)}px`);
    publish();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(publish);
    observer?.observe(surface);
    return () => { observer?.disconnect(); root.style.removeProperty("--ct-composer-height"); };
  }, [ref]);
}
