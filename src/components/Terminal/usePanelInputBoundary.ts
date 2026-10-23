import { useLayoutEffect, useRef } from "react";

export function usePanelInputBoundary(isVisible: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const panel = ref.current;
    const focused = panel?.ownerDocument.activeElement;
    if (!isVisible && focused instanceof HTMLElement && panel?.contains(focused)) focused.blur();
  }, [isVisible]);
  return ref;
}
