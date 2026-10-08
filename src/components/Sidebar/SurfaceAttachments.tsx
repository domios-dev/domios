import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { Tab } from "../../lib/types";
import { useKeyedFlag } from "../../lib/keyedFlagStore";
import { useSurfacePulse } from "../../lib/surfaceActivity";
import { useSurfaceControlled } from "../../lib/surfaceControlStore";
import { attachmentBlockModel, attachmentCollapseStore } from "./surfaceAttachmentModel";
import { relativeRect, tetherPaths } from "./tetherGeometry";
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
  const collapsed = useKeyedFlag(attachmentCollapseStore, ownerId);
  const onToggle = useCallback(() => attachmentCollapseStore.set(ownerId, !attachmentCollapseStore.get(ownerId)), [ownerId]);
  if (attachments.length === 0) return <>{renderOwner(null)}</>;
  const model = attachmentBlockModel(attachments, collapsed, compact);
  return (
    <div className="sidebar-surface-owner" data-testid="surface-owner-block" data-owner-id={ownerId}>
      <div className="sidebar-surface-owner-row" data-tether-owner="">
        {renderOwner({ count: attachments.length, collapsed, onToggle })}
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

function sameStrings(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

function measurePaths(block: Element, attachmentIds: readonly string[]): string[] {
  const owner = block.querySelector(`[data-tether-owner] ${ICON_SELECTOR}`);
  if (!owner) return attachmentIds.map(() => "");
  const rows = new Map([...block.querySelectorAll("[data-tether-attachment]")].map((row) => [row.getAttribute("data-tether-attachment"), row]));
  const rects = attachmentIds.map((id) => {
    const icon = rows.get(id)?.querySelector(ICON_SELECTOR);
    return icon ? relativeRect(icon, block) : { left: 0, top: 0, width: 0, height: 0 };
  });
  return tetherPaths(relativeRect(owner, block), rects);
}

function SurfaceTether({ attachmentIds }: { attachmentIds: readonly string[] }) {
  const [paths, setPaths] = useState<string[]>([]);
  // The block's own ref attaches after this child's layout effect; the svg's is already set.
  const svgRef = useRef<SVGSVGElement>(null);
  const idsKey = attachmentIds.join(ID_SEPARATOR);
  useLayoutEffect(() => {
    const block = svgRef.current?.parentElement;
    if (!block) return;
    const ids = idsKey.split(ID_SEPARATOR);
    const update = () => {
      const next = measurePaths(block, ids);
      setPaths((current) => (sameStrings(current, next) ? current : next));
    };
    update();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(update);
    observer.observe(block);
    return () => observer.disconnect();
  }, [idsKey]);
  return (
    <>
      <svg ref={svgRef} className="sidebar-tether" aria-hidden="true" focusable="false">
        {attachmentIds.map((id, index) => (paths[index] ? <TetherStrand key={id} surfaceTabId={id} d={paths[index]} /> : null))}
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
