import type { CSSProperties } from "react";

export type CanvasLod = "far" | "mid" | "near";

export interface CanvasCardStyleProps {
  style: CSSProperties;
  lod?: CanvasLod;
}
