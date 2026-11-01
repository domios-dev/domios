import type { CSSProperties, ReactNode } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";

export type SkeletonShape = "line" | "circle" | "tile";

const SHAPE_CLASS: Record<SkeletonShape, string> = {
  line: "rounded",
  circle: "rounded-full",
  tile: "rounded-[var(--ct-radius-md)]",
};

/** One placeholder shape; sizes are any CSS length so callers mirror the layout they stand in for. */
export function SkeletonBlock({ shape = "line", width, height, className, faint = false }: {
  shape?: SkeletonShape;
  width?: CSSProperties["width"];
  height?: CSSProperties["height"];
  className?: string;
  faint?: boolean;
}) {
  return (
    <span
      aria-hidden="true"
      className={`ct-skeleton-block block shrink-0 ${SHAPE_CLASS[shape]} ${faint ? "opacity-60" : ""} ${className ?? ""}`}
      style={{ width, height }}
    />
  );
}

/** Groups placeholder blocks under one breathing animation and one hidden-from-readers boundary. */
export function Skeleton({ children, className, inline = false, "data-testid": testId }: {
  children: ReactNode;
  className?: string;
  /** Renders a span so the skeleton can sit inside buttons and other phrasing content. */
  inline?: boolean;
  "data-testid"?: string;
}) {
  return inline ? (
    <span aria-hidden="true" data-testid={testId} className={`ct-skeleton inline-flex ${className ?? ""}`}>
      {children}
    </span>
  ) : (
    <DeferredLoader state="present" aria-hidden="true" data-testid={testId} className={`ct-skeleton ${className ?? ""}`}>
      {children}
    </DeferredLoader>
  );
}
