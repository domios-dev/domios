import { useLayoutEffect, useRef } from "react";
import { type ViewportBox, type ViewportTransform } from "@codeterm/canvas-camera";
import { useCanvasStage } from "@codeterm/chat-ui/useCanvasStage";
import { frameBlueprint, type BlueprintLayout } from "./layout";

export interface AutomationCamera {
  transform: ViewportTransform | null;
  userMoved: boolean;
  overview: boolean;
  focus: string | null;
  selected: string | null;
}

export function createAutomationCamera(): AutomationCamera {
  return { transform: null, userMoved: false, overview: false, focus: null, selected: null };
}

export function useAutomationViewport(layout: BlueprintLayout, camera: AutomationCamera, focus: string | null, panelOpen = false, onPaint?: (transform: ViewportTransform) => void) {
  const overview = useRef(camera.overview);
  const framedFocus = useRef(camera.focus);
  const stage = useCanvasStage({
    onReset: () => settle(false, focus),
    memory: camera, ignoreSelector: "button", wheelBehavior: "zoom", onPaint,
    minScale: size => size.width > 0 && size.width < 760 ? 0.5 : 0.05,
    followFit: true, fitRevision: `${layout.bounds.width}:${layout.bounds.height}`,
    fit: (viewport, size) => {
      const transform = frameBlueprint(layout, size, overview.current, framedFocus.current);
      if (!transform) return false;
      viewport.setTransform(transform);
    },
  });
  const { camera: viewport, size } = stage;
  const settle = (whole: boolean, target: string | null) => {
    overview.current = whole;
    framedFocus.current = target;
    stage.fit();
    camera.overview = whole;
    camera.focus = target;
  };
  const panelWasOpen = useRef(false);
  useLayoutEffect(() => {
    if (panelOpen && !panelWasOpen.current && stage.measured && !stage.userMoved()) settle(false, focus);
    panelWasOpen.current = panelOpen && stage.measured;
  });
  const reveal = (box: ViewportBox) => {
    if (!stage.measured) return;
    const t = viewport.transformRef.current;
    const left = box.x * t.scale + t.x, top = box.y * t.scale + t.y;
    if (left >= 0 && top >= 0 && left + box.width * t.scale <= size.width && top + box.height * t.scale <= size.height) return;
    stage.takeControl();
    viewport.setTransform({ ...t, x: size.width / 2 - (box.x + box.width / 2) * t.scale, y: size.height / 2 - (box.y + box.height / 2) * t.scale });
  };
  return { ...viewport, measured: stage.measured, fit: () => settle(false, focus), showOverview: () => settle(true, null), zoom: stage.zoom, reveal, interactions: stage.interactions };
}
