export const RENDERING = {
  cullThresholdNodes: 200,
  cullPadding: 100,
  domOnlyAboveScale: 1.1,
  webglOnlyBelowScale: 0.9,
  webglOnlyNodeCount: 500,
  forceWebglDuringFadeAboveCount: 1000,
} as const;

export function computeRendererOpacities(scale: number, nodeCount: number): {
  domOpacity: number;
  webglOpacity: number;
} {
  let domOpacity = Math.max(0, Math.min(1, (scale - RENDERING.webglOnlyBelowScale) / 0.2));
  if (scale < RENDERING.webglOnlyBelowScale && nodeCount > RENDERING.webglOnlyNodeCount) {
    domOpacity = 0;
  }
  if (
    scale >= RENDERING.webglOnlyBelowScale &&
    scale <= RENDERING.domOnlyAboveScale &&
    nodeCount > RENDERING.forceWebglDuringFadeAboveCount
  ) {
    domOpacity = 0;
  }
  return { domOpacity, webglOpacity: 1 - domOpacity };
}
