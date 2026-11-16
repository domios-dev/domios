import { useEffect, useRef } from "react";
import { recordProductEvent } from "../lib/productAnalytics";

export function useProductViewEvent(event: "canvas_opened") {
  const recorded = useRef(false);
  useEffect(() => {
    if (recorded.current) return;
    recorded.current = true;
    void recordProductEvent({ event_type: event, event_properties: {} });
  }, [event]);
}
