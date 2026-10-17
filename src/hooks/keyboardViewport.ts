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
  const keyboardOpen = geometry.layoutHeight - visualHeight - offsetTop > 100;
  return {
    inset: keyboardOpen ? Math.max(0, Math.round(shellGap)) : 0,
    keyboardOpen,
    visualHeight,
    offsetTop,
  };
}
