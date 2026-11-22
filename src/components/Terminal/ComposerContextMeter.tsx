import { memo, useCallback, useEffect, useId, useRef, useState } from "react";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { classify } from "../../types/usage";
import { contextMeter } from "./composerModels";
import { useContextClaimed } from "../Usage/contextUsage";
import { UsageGauge } from "../Usage/UsageGauge";
import { AnchoredPopover } from "../shared/AnchoredPopover";

export const ComposerContextMeter = memo(function ComposerContextMeter({ tabId }: { tabId: string }) {
  const context = useMuxSelector(useCallback((state) => {
    const tab = state.tabs[tabId];
    return tab && "context" in tab ? tab.context : undefined;
  }, [tabId]));
  const meter = contextMeter(context);
  const claimed = useContextClaimed(tabId);
  if (!meter || claimed) return null;
  return <ContextDetails key={tabId} meter={meter} />;
});

function ContextDetails({ meter }: { meter: NonNullable<ReturnType<typeof contextMeter>> }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const dialogId = useId();
  const close = useCallback(() => setOpen(false), []);
  useEffect(() => {
    if (open) closeButton.current?.focus({ preventScroll: true });
  }, [open]);
  const percent = Math.round(meter.percent);
  return <>
    <button ref={anchor} type="button" className="ct-bar-pill" aria-label={`Context ${percent}%`}
      aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? dialogId : undefined}
      onClick={(event) => { event.stopPropagation(); setOpen(!open); }}>
      <meter className="ct-bar-meter" aria-hidden="true" min={0} max={meter.window} value={meter.used}>{percent}%</meter>
      {percent}%
    </button>
    <AnchoredPopover open={open} anchor={anchor} placement="bottom-end" className="ct-usage" onClose={close}>
      <div id={dialogId} className="ct-usage-popover" role="dialog" aria-label="Context window">
        <div className="head"><span className="title">Context window</span></div>
        <UsageGauge label="Context used" variant="session" pct={meter.percent} resetsAt={null} state={classify(meter.percent)}
          now={Date.now()} note={`${meter.used.toLocaleString()} of ${meter.window.toLocaleString()} tokens`} />
        <p className="ct-usage-detail">As the window fills, the agent may compact older messages into a summary to make room.</p>
        <div className="ct-usage-foot">
          <button ref={closeButton} type="button" className="link" onClick={() => {
            close();
            anchor.current?.focus({ preventScroll: true });
          }}>Close</button>
        </div>
      </div>
    </AnchoredPopover>
  </>;
}
