import { useLayoutEffect, useState } from "react";
import { CHAT_TOOL_SHOW_DELAY_MS } from "./chatTranscriptActivity";

export function useBackgroundProcessChipVisibility(tabId: string, defer: boolean): boolean {
  const [visibleFor, setVisibleFor] = useState<string | null>(null);
  useLayoutEffect(() => {
    setVisibleFor(null);
    if (!defer) return;
    const timer = window.setTimeout(() => setVisibleFor(tabId), CHAT_TOOL_SHOW_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [defer, tabId]);
  return !defer || visibleFor === tabId;
}
