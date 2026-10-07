import { useCallback } from "react";
import { useMuxSelector } from "../../store/multiplexer";
import { contextMeter } from "./composerModels";

export function ComposerContextMeter({ tabId }: { tabId: string }) {
  const context = useMuxSelector(useCallback((state) => {
    const tab = state.tabs[tabId];
    return tab && "context" in tab ? tab.context : undefined;
  }, [tabId]));
  const meter = contextMeter(context);
  if (!meter) return null;
  return <span className="inline-flex items-center gap-2 text-xs" title={`${meter.used.toLocaleString()} / ${meter.window.toLocaleString()} tokens`}>
    <meter aria-label="Context window" min={0} max={meter.window} value={meter.used}>{Math.round(meter.percent)}%</meter>
    {Math.round(meter.percent)}%
  </span>;
}
