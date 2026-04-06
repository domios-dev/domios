import { useEffect } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

export interface UseTauriFileDropOptions {
  containerRef: React.RefObject<HTMLElement | null>;
  onPaths: (paths: string[]) => void;
  enabled?: boolean;
}

/** Subscribe to Tauri v2 native drag-drop; invoke callback when drop lands inside containerRef. */
export function useTauriFileDrop({
  containerRef,
  onPaths,
  enabled = true,
}: UseTauriFileDropOptions): void {
  useEffect(() => {
    if (!enabled) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const el = containerRef.current;
      if (!el) return;
      if (!isDropInside(event.payload.position, el)) return;
      const paths = event.payload.paths || [];
      if (paths.length > 0) onPaths(paths);
    }).then((fn) => {
      if (cancelled) fn(); else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [enabled, containerRef, onPaths]);
}

function isDropInside(
  position: { x: number; y: number },
  el: HTMLElement,
): boolean {
  // Tauri reports physical pixels; convert to CSS via DPR, then hit-test via DOM.
  // elementFromPoint handles all layout/transform/overlay cases correctly.
  const dpr = window.devicePixelRatio || 1;
  const target = document.elementFromPoint(position.x / dpr, position.y / dpr);
  return target !== null && (target === el || el.contains(target));
}
