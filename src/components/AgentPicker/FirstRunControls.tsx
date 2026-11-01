import { useLayoutEffect, useRef, type ReactNode, type SyntheticEvent } from "react";
import { FIRST_RUN_DISABLED_REASON } from "../../lib/firstRunGate";

const controls = "button, input, select, textarea, a, [tabindex], [role=button], [role=switch]";
const allowed = "[data-first-run-allowed=true]";
const dragRegion = "[data-tauri-drag-region]";

function useFirstRunControls(locked: boolean) {
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!locked || !root.current) return;
    const previous = new Map<HTMLElement, Map<string, string | null>>();
    let focusedHome = false;
    const disable = () => {
      for (const control of root.current!.querySelectorAll<HTMLElement>(controls)) {
        if (control.closest(allowed) || previous.has(control)) continue;
        const attributes = new Map(["disabled", "aria-disabled", "tabindex", "title"].map((name) => [name, control.getAttribute(name)]));
        previous.set(control, attributes);
        if (control.matches("button, input, select, textarea")) control.setAttribute("disabled", "");
        control.setAttribute("aria-disabled", "true");
        control.setAttribute("tabindex", "-1");
        control.setAttribute("title", FIRST_RUN_DISABLED_REASON);
      }
      const home = root.current!.querySelector<HTMLElement>("[data-first-run-home=true]");
      if (home && !focusedHome) {
        focusedHome = true;
        home.focus();
      }
    };
    disable();
    const observer = new MutationObserver(disable);
    observer.observe(root.current, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      for (const [control, attributes] of previous) {
        for (const [name, value] of attributes) {
          if (value === null) control.removeAttribute(name);
          else control.setAttribute(name, value);
        }
      }
    };
  }, [locked]);
  const guard = (event: SyntheticEvent) => {
    if (!locked) return;
    if (event.target instanceof Node && !root.current?.contains(event.target)) return;
    // The window still moves and maximizes from its title bar: a drag region outside any control passes through.
    if (event.target instanceof Element && event.target.closest(dragRegion) && !event.target.closest(controls)) return;
    if (event.target instanceof Element && event.target.closest(allowed) && event.type !== "contextmenu" && event.type !== "dblclick") {
      if (["pointerdown", "mousedown", "touchstart"].includes(event.type) && event.target.closest("[data-first-run-home=true]")) event.stopPropagation();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  };
  return { root, guard };
}

export function FirstRunControls({ locked, children }: { locked: boolean; children: ReactNode }) {
  const { root, guard } = useFirstRunControls(locked);
  return <div ref={root} style={{ display: "contents" }} onClickCapture={guard} onPointerDownCapture={guard}
    onMouseDownCapture={guard} onTouchStartCapture={guard}
    onDoubleClickCapture={guard} onContextMenuCapture={guard} onKeyDownCapture={guard}>{children}</div>;
}
