import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "../../lib/bridge";

export interface UseTauriFileDropOptions {
  containerRef: React.RefObject<HTMLElement | null>;
  onPaths: (paths: string[]) => void;
  enabled?: boolean;
}

export interface UseTauriFileDropReturn {
  /** True while the OS is dragging a file over THIS container. Drives the
   *  drop-here visual overlay. Resets on leave or drop. */
  isDragOver: boolean;
}

/** Subscribe to Tauri v2 native drag-drop. Invokes `onPaths` when a drop
 *  lands inside `containerRef`. Reports a live `isDragOver` flag while the
 *  dragged file's cursor is hovering this container so the UI can show
 *  a drop-target overlay. */
export function useTauriFileDrop({
  containerRef,
  onPaths,
  enabled = true,
}: UseTauriFileDropOptions): UseTauriFileDropReturn {
  const onPathsRef = useRef(onPaths);
  onPathsRef.current = onPaths;
  const [isDragOver, setIsDragOver] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    // Native drag-drop is a Tauri webview feature — no-op in web mode.
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      const el = containerRef.current;
      if (!el) return;

      if (payload.type === "drop") {
        setIsDragOver(false);
        if (!isDropInside(payload.position, el)) return;
        const paths = payload.paths || [];
        if (paths.length > 0) {
          void materializeDroppedPaths(paths).then((materialized) => {
            onPathsRef.current(materialized);
          });
        }
        return;
      }
      if (payload.type === "leave") {
        setIsDragOver(false);
        return;
      }
      // "enter" and "over" both carry position — flip the flag based on whether the cursor is currently inside this specific container. Without the
      // per-container check, every pane would highlight at once when one file is dragged into the window.
      if (payload.type === "enter" || payload.type === "over") {
        setIsDragOver(isDropInside(payload.position, el));
      }
    }).then((fn) => {
      if (cancelled) fn(); else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
      setIsDragOver(false);
    };
  }, [enabled, containerRef]);

  return { isDragOver };
}

export async function materializeDroppedPaths(paths: string[]): Promise<string[]> {
  if (paths.length === 0) return [];
  try {
    return await invoke<string[]>("materialize_dropped_files", { paths });
  } catch {
    return paths;
  }
}

function isDropInside(
  position: { x: number; y: number },
  el: HTMLElement,
): boolean {
  const dpr = window.devicePixelRatio || 1;
  const target = document.elementFromPoint(position.x / dpr, position.y / dpr);
  return target !== null && (target === el || el.contains(target));
}
