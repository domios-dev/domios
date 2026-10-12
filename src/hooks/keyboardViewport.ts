export interface KeyboardViewportGeometry {
  rootHeight: number;
  layoutHeight: number;
  visualHeight: number;
  offsetTop: number;
}

export function keyboardViewportState(geometry: KeyboardViewportGeometry) {
  const visualHeight = Math.max(0, geometry.visualHeight);
  const offsetTop = Math.max(0, geometry.offsetTop);
  const shellGap = geometry.rootHeight - visualHeight - offsetTop;
  return {
    inset: shellGap > 100 ? Math.round(shellGap) : 0,
    keyboardOpen: geometry.layoutHeight - visualHeight - offsetTop > 100,
    visualHeight,
    offsetTop,
  };
}
