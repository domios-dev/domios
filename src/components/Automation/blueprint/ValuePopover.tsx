import { useState, type PointerEvent as ReactPointerEvent } from "react";
import { Check, Copy } from "lucide-react";
import { previewText, type PinValue } from "./pinValues";

/** A data pin or wire being inspected: where it sits on the canvas and what flowed through it. */
export interface ValueProbe { key: string; x: number; y: number; side: "in" | "out"; port: string; type?: string; value: PinValue }

export const VALUE_POPOVER_CLASS = "bp-value";

function size(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} kB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** The recorded value of one port: a pretty preview with Copy, the run's summary, or an honest "nothing recorded". */
export function PinValueBody({ value, type }: { value: PinValue; type?: string }) {
  const [copied, setCopied] = useState(false);
  const recorded = value.kind === "value" ? value.value : null;
  const shown = recorded ? previewText(recorded.preview) : null;
  const copy = async () => {
    if (!recorded) return;
    try { await navigator.clipboard.writeText(recorded.preview); setCopied(true); } catch { setCopied(false); }
  };
  const facts = recorded ? [recorded.type ?? type, recorded.bytes !== undefined ? size(recorded.bytes) : null, recorded.truncated || shown?.cut ? "preview truncated" : null, recorded.redacted ? "redacted" : null].filter(Boolean) : [];
  return <>
    {value.kind === "value" && shown && <pre className="bp-value__preview">{shown.text}</pre>}
    {value.kind === "summary" && <p className="bp-value__summary">{value.text}<span>Only a summary was recorded for this run.</span></p>}
    {value.kind === "omitted" && <p className="bp-value__empty">This run's values were too large to record.</p>}
    {value.kind === "none" && <p className="bp-value__empty">No value recorded for this run.</p>}
    {recorded && <div className="bp-value__foot">
      <span>{facts.join(" · ")}</span>
      <button type="button" onClick={() => void copy()}>{copied ? <Check size={12} aria-hidden="true" /> : <Copy size={12} aria-hidden="true" />}{copied ? "Copied" : "Copy value"}</button>
    </div>}
  </>;
}

export function ValuePopover({ probe, onLeave }: { probe: ValueProbe; onLeave: (event: ReactPointerEvent) => void }) {
  return <div role="group" aria-label={`${probe.side === "in" ? "Input" : "Output"} ${probe.port} in this run`} className={VALUE_POPOVER_CLASS} data-side={probe.side}
    style={{ left: probe.x, top: probe.y }} onPointerLeave={onLeave} onMouseDown={(event) => event.stopPropagation()}>
    <p className="bp-value__head"><span>{probe.side === "in" ? "Received" : "Produced"}</span><code>{probe.port}</code>{probe.type && <span className="bp-value__type">{probe.type}</span>}</p>
    <PinValueBody value={probe.value} type={probe.type} />
  </div>;
}
