import type { HTMLAttributes, ReactNode } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";

/**
 * Loading in Domios: which one to use where. There are exactly three, and no other loading component is added.
 *
 * 1. `Loader` (this module): a whole area is waiting for its content, such as the app booting, a workspace or tab
 *    restoring, a view, a pane, a panel region, a conversation or a lazy view. It is the branded loader: the mark over
 *    the spreading dot matrix, with a caption. Variants:
 *    - `fullscreen` covers the window (the boot cover's hand-over);
 *    - `overlay` covers its positioned parent, over content that stays mounted (a graph, a view);
 *    - `region` (the default) sits in the flow, fills its container and holds at least a comfortable height, so the
 *      dots have room;
 *    - `compact` is the same with a smaller mark, for small panels and dialogs.
 *    It is deferred by DeferredLoader, so a fast load never flashes it; pass `deferred={false}` only where a cover
 *    is already on screen and must hand over without a gap.
 * 2. The activity dots: something small waits inside content that stays, such as a button, a row, a status line or
 *    a menu. Use `BusyState` (dots with an accessible label, an optional caption, `inline`, `hideLabel`) or a Button's
 *    `loading`; `ActivityDots` is their primitive.
 * 3. `Skeleton`: a placeholder in the shape of content about to appear, where the layout is known. It is not a loader
 *    and carries its own hidden status text.
 *
 * A control that spins its own icon while its action runs (Refresh) uses BusySpinner; that is the control, not a loader.
 */
export type LoaderVariant = "fullscreen" | "overlay" | "region" | "compact";

const VARIANT_BOX: Record<LoaderVariant, string> = {
  fullscreen: "fixed inset-0 z-[60]",
  overlay: "absolute inset-0",
  region: "relative h-full w-full min-h-48",
  compact: "relative h-full w-full min-h-28",
};

export function Loader({ label, detail, variant = "region", deferred = true, animate = true, pulse = false, surfaceClassName, className, children, ...attributes }: {
  label: string;
  /** One quieter line under the caption. */
  detail?: string;
  variant?: LoaderVariant;
  deferred?: boolean;
  /** False keeps the dots still, for a state that is waiting rather than working. */
  animate?: boolean;
  /** Beat the mark and release dot waves from it. */
  pulse?: boolean;
  /** Classes for the branded surface itself, such as a material. */
  surfaceClassName?: string;
  /** Below the caption, such as a progress strip. */
  children?: ReactNode;
} & HTMLAttributes<HTMLDivElement>) {
  // The surface fills a positioned box, so the dot matrix always has the box's full size to spread across.
  const surface = <BrandState model={brandStateModel({ loading: true, label, detail, animate })} pulse={pulse}
    markSize={variant === "compact" ? 48 : 92} className={`absolute inset-0 ${variant === "compact" ? "brand-state--compact" : ""} ${surfaceClassName ?? ""}`}>
    {children}
  </BrandState>;
  const box = `${VARIANT_BOX[variant]} ${className ?? ""}`.trim();
  return deferred
    ? <DeferredLoader state="present" className={box} {...attributes}>{surface}</DeferredLoader>
    : <div className={box} {...attributes}>{surface}</div>;
}
