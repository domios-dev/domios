export interface ChatGeometryAncestor {
  hidden: boolean;
  inert: boolean;
  ariaHidden: boolean;
  display: string;
  visibility: string;
  opacity: number;
}

export function chatGeometryVisible(width: number, height: number, ancestors: readonly ChatGeometryAncestor[]): boolean {
  return width > 0 && height > 0 && ancestors.every((ancestor) =>
    !ancestor.hidden && !ancestor.inert && !ancestor.ariaHidden && ancestor.display !== "none"
      && ancestor.visibility === "visible" && ancestor.opacity > 0);
}

export interface ChatGeometrySurface {
  tabId: string | null;
  visible: boolean;
  snapshotReady: boolean;
  busy: boolean;
}

export function selectChatGeometrySurface(surfaces: readonly ChatGeometrySurface[]): number | null {
  const eligible = surfaces.flatMap((surface, index) =>
    surface.tabId && surface.visible && surface.snapshotReady && !surface.busy ? [index] : []);
  return eligible.length === 1 ? eligible[0] : null;
}
