import { useEffect, useState, type CSSProperties } from "react";
import { AnchoredPopoverHost } from "../../../../packages/chat-ui/src/AnchoredPopover";
import { Workspace, type WorkspaceController, type WorkspaceLayout } from "../workspace/Workspace";
import type { PlaceId } from "../workspace/nav";
import { STAGE_VH, STAGE_VW } from "./rootClass";
import { setStage } from "./scope";

export type { PlaceId, WorkspaceController, WorkspaceLayout };

export interface WorkspaceCardProps {
  place: PlaceId;
  layout: WorkspaceLayout;
  reducedMotion: boolean;
  tour: boolean;
  controller: WorkspaceController;
  onPlace: (place: PlaceId) => void;
}

/**
 * The live workspace at the slot's real size, never scaled: the product's width queries answer from this box, so the
 * phone slot gets the app's own narrow layout at real type sizes.
 */
export function WorkspaceCard({ place, layout, reducedMotion, tour, controller, onPlace }: WorkspaceCardProps) {
  const [box, setBox] = useState<HTMLDivElement | null>(null);
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  useEffect(() => {
    if (!box) return;
    // Only a real change re-renders, so the product's own layout reacting to it never loops the observer.
    const observer = new ResizeObserver(() => {
      const next = { width: box.clientWidth, height: box.clientHeight };
      setSize((prev) => (prev && prev.width === next.width && prev.height === next.height ? prev : next));
    });
    observer.observe(box);
    return () => observer.disconnect();
  }, [box]);
  if (size) setStage(size.width, size.height);
  const vars = (size ? { [STAGE_VW]: `${size.width / 100}px`, [STAGE_VH]: `${size.height / 100}px` } : {}) as CSSProperties;
  return (
    <div ref={setBox} className="film-workspace" data-reduced-motion={reducedMotion || undefined} style={vars}>
      <AnchoredPopoverHost value={box}>
      {size && size.width > 0 && (
        <Workspace place={place} layout={layout} reducedMotion={reducedMotion} tour={tour} controller={controller} onPlace={onPlace} />
      )}
      </AnchoredPopoverHost>
    </div>
  );
}
