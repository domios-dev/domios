import { useId, useState, type ReactNode } from "react";
import { ChevronDown, Info } from "lucide-react";
import { blueprintLegend, blueprintNodeKinds, type BlueprintNodeType } from "./kinds";

const OPEN_KEY = "domios.blueprint.info-open";

function readOpen(): boolean {
  try { return window.localStorage.getItem(OPEN_KEY) !== "false"; } catch { return true; }
}

function writeOpen(open: boolean) {
  try { window.localStorage.setItem(OPEN_KEY, String(open)); } catch { return; }
}

/** The canvas corner that holds what used to stack above it: run notices stay visible, notes and the legend fold away. */
export function CanvasOverlay({ types, info, notices }: { types: ReadonlySet<BlueprintNodeType> | null; info?: ReactNode; notices?: ReactNode }) {
  const [open, setOpen] = useState(readOpen);
  const bodyId = useId();
  const legend = types ? blueprintLegend.filter((type) => types.has(type)) : [];
  const toggle = () => { setOpen(!open); writeOpen(!open); };
  return <div className="bp-canvas-notices" onMouseDown={(event) => event.stopPropagation()}>
    {notices}
    {(info || legend.length > 0) && <section className="bp-info" aria-label="Blueprint info" data-open={open || undefined}>
      <button type="button" className="bp-info__toggle" aria-expanded={open} aria-controls={bodyId} onClick={toggle}>
        <Info size={13} aria-hidden="true" /><span>Info</span><ChevronDown size={13} className="bp-info__chevron" aria-hidden="true" />
      </button>
      <div id={bodyId} className="bp-info__body" hidden={!open}>
        {info}
        {legend.length > 0 && <ul aria-label="Node types" className="bp-legend">
          {legend.map((type) => <li key={type} data-type={type}><span className="bp-legend__swatch" aria-hidden="true" />{blueprintNodeKinds[type].label}</li>)}
        </ul>}
      </div>
    </section>}
  </div>;
}
