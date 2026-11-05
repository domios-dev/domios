import { memo, useCallback } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { contextMeter } from "./composerModels";
import { useContextClaimed } from "../Usage/contextUsage";

export const ComposerContextMeter = memo(function ComposerContextMeter({ tabId }: { tabId: string }) {
  const context = useMuxSelector(useCallback((state) => {
    const tab = state.tabs[tabId];
    return tab && "context" in tab ? tab.context : undefined;
  }, [tabId]));
  const meter = contextMeter(context);
  // The usage bubble already shows this fill for a harness without quota windows.
  const claimed = useContextClaimed(tabId);
  if (!meter || claimed) return null;
  return <span className="ct-bar-pill ct-bar-pill--static" title={`${meter.used.toLocaleString()} / ${meter.window.toLocaleString()} tokens`}>
    <meter className="ct-bar-meter" aria-label="Context window" min={0} max={meter.window} value={meter.used}>{Math.round(meter.percent)}%</meter>
    {Math.round(meter.percent)}%
  </span>;
});
