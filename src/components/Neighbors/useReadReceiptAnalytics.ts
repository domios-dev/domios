import { useEffect, useRef, type RefObject } from "react";
import { recordProductEvent } from "../../lib/productAnalytics";

export function useReadReceiptAnalytics(element: RefObject<HTMLElement | null>, read: boolean) {
  const shown = useRef(false);
  useEffect(() => {
    if (!read || shown.current || !element.current || typeof IntersectionObserver === "undefined") return;
    let visible = false;
    const record = () => {
      if (visible && document.visibilityState === "visible" && !shown.current) {
        shown.current = true;
        void recordProductEvent({ event_type: "read_receipt_shown", event_properties: { count: 1 } });
      }
    };
    const observer = new IntersectionObserver(entries => {
      visible = entries.some(entry => entry.isIntersecting);
      record();
    }, { threshold: 0.5 });
    observer.observe(element.current);
    document.addEventListener("visibilitychange", record);
    return () => { observer.disconnect(); document.removeEventListener("visibilitychange", record); };
  }, [element, read]);
}
