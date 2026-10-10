import { useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { Tab } from "../../lib/types";
import { useSurfacePulse } from "../../lib/surfaceActivity";
import { useSurfaceControlled } from "../../lib/surfaceControlStore";
import { attachmentBlockModel } from "./surfaceAttachmentModel";
import { relativeRect, tetherClearings, tetherPaths, type TetherClearing, type TetherRect } from "./tetherGeometry";
import type { AttachmentToggle } from "./TabItem";

const ICON_SELECTOR = '[data-testid="tab-tab-icon"]';
const ID_SEPARATOR = "\u0000";

interface SurfaceOwnerBlockProps {
  ownerId: string;
  attachments: readonly Tab[];
  compact: boolean;
  renderOwner: (toggle: AttachmentToggle | null) => ReactNode;
  renderAttachment: (tab: Tab, index: number) => ReactNode;
  wrapAttachments?: (rows: ReactNode) => ReactNode;
}

/** An owner row with its surfaces tucked under it, joined by a tether rather than a tree connector. */
export function SurfaceOwnerBlock({ ownerId, attachments, compact, renderOwner, renderAttachment, wrapAttachments = (rows) => rows }: SurfaceOwnerBlockProps) {
  if (attachments.length === 0) return <>{renderOwner(null)}</>;
  const model = attachmentBlockModel(attachments, false, compact);
  return (
    <div className="sidebar-surface-owner" data-testid="surface-owner-block" data-owner-id={ownerId}>
      <div className="sidebar-surface-owner-row" data-tether-owner="">
        {renderOwner(null)}
      </div>
      {model.rows.length > 0 && (
        <div className="sidebar-surface-attachments" data-testid="surface-attachments">
          {wrapAttachments(model.rows.map((tab, index) => (
            <div key={tab.id} className="sidebar-surface-attachment" data-tether-attachment={tab.id}>
              {renderAttachment(tab, index)}
            </div>
          )))}
        </div>
      )}
      {model.tethered && <SurfaceTether attachmentIds={model.rows.map((tab) => tab.id)} />}
    </div>
  );
}

interface TetherLayout {
  paths: string[];
  clearings: TetherClearing[];
}

const EMPTY_LAYOUT: TetherLayout = { paths: [], clearings: [] };
const UNMEASURED: TetherRect = { left: 0, top: 0, width: 0, height: 0 };

function sameLayout(a: TetherLayout, b: TetherLayout): boolean {
  return (
    a.paths.length === b.paths.length &&
    a.paths.every((value, index) => value === b.paths[index]) &&
    a.clearings.length === b.clearings.length &&
    a.clearings.every((disc, index) => disc.cx === b.clearings[index].cx && disc.cy === b.clearings[index].cy && disc.r === b.clearings[index].r)
  );
}

function measureLayout(block: Element, attachmentIds: readonly string[]): TetherLayout {
  const owner = block.querySelector(`[data-tether-owner] ${ICON_SELECTOR}`);
  if (!owner) return { paths: attachmentIds.map(() => ""), clearings: [] };
  const rows = new Map([...block.querySelectorAll("[data-tether-attachment]")].map((row) => [row.getAttribute("data-tether-attachment"), row]));
  const rects = attachmentIds.map((id) => {
    const icon = rows.get(id)?.querySelector(ICON_SELECTOR);
    return icon ? relativeRect(icon, block) : UNMEASURED;
  });
  const ownerRect = relativeRect(owner, block);
  return { paths: tetherPaths(ownerRect, rects), clearings: tetherClearings(rects) };
}

function SurfaceTether({ attachmentIds }: { attachmentIds: readonly string[] }) {
  const [layout, setLayout] = useState<TetherLayout>(EMPTY_LAYOUT);
  // The block's own ref attaches after this child's layout effect; the svg's is already set.
  const svgRef = useRef<SVGSVGElement>(null);
  const maskId = `sidebar-tether-mask-${useId().replace(/:/g, "")}`;
  const idsKey = attachmentIds.join(ID_SEPARATOR);
  useLayoutEffect(() => {
    const block = svgRef.current?.parentElement;
    if (!block) return;
    const ids = idsKey.split(ID_SEPARATOR);
    const update = () => {
      const next = measureLayout(block, ids);
      setLayout((current) => (sameLayout(current, next) ? current : next));
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(block);
    return () => observer.disconnect();
  }, [idsKey]);
  const { paths, clearings } = layout;
  return (
    <>
      <svg ref={svgRef} className="sidebar-tether" aria-hidden="true" focusable="false">
        {/* userSpaceOnUse: a straight drop has a zero-width bbox, which would hide it under objectBoundingBox. */}
        <mask id={maskId} maskUnits="userSpaceOnUse" x="0" y="0" width="100%" height="100%">
          <rect x="0" y="0" width="100%" height="100%" fill="white" />
          {clearings.map((disc, index) => <circle key={index} data-testid="surface-tether-clearing" cx={disc.cx} cy={disc.cy} r={disc.r} fill="black" />)}
        </mask>
        <g mask={`url(#${maskId})`}>
          {attachmentIds.map((id, index) => (paths[index] ? <TetherStrand key={id} surfaceTabId={id} d={paths[index]} /> : null))}
        </g>
      </svg>
      {attachmentIds.map((id, index) => (paths[index] ? <TetherDroplet key={id} surfaceTabId={id} d={paths[index]} /> : null))}
    </>
  );
}

function TetherStrand({ surfaceTabId, d }: { surfaceTabId: string; d: string }) {
  const controlled = useSurfaceControlled(surfaceTabId);
  const pulse = useSurfacePulse(surfaceTabId);
  const state = controlled ? "controlled" : pulse ? "driving" : "idle";
  // Keyed by the pass so the reduced-motion fade restarts on every verb.
  return <path key={pulse?.nonce ?? 0} className="sidebar-tether-path" data-testid="surface-tether" data-state={state} data-surface-tab-id={surfaceTabId} d={d} />;
}

function TetherDroplet({ surfaceTabId, d }: { surfaceTabId: string; d: string }) {
  const controlled = useSurfaceControlled(surfaceTabId);
  const pulse = useSurfacePulse(surfaceTabId);
  if (controlled || !pulse) return null;
  return <span key={pulse.nonce} className="sidebar-tether-droplet" data-testid="surface-tether-droplet" style={{ offsetPath: `path("${d}")` }} />;
}
