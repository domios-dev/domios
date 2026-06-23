import { useState, type DragEvent } from "react";
import { IS_WEB } from "../../lib/webApi";
import { storeBlob } from "./voice/audio";

export async function storeBrowserFiles(
  files: Iterable<File>,
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
): Promise<void> {
  const paths: string[] = [];
  for (const f of files) {
    try {
      const path = await storeBlob(f, f.name || "file.bin");
      if (path.trim()) paths.push(path);
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  }
  if (paths.length > 0) onAttach(paths);
}

export function useWebFileDrop(
  onAttach: (paths: string[]) => void,
  onError: (msg: string) => void,
) {
  const [dragOver, setDragOver] = useState(false);
  if (!IS_WEB) {
    return { dragOver: false, handlers: {} };
  }
  const handlers = {
    onDragOver: (e: DragEvent) => {
      if (e.dataTransfer.types.includes("Files")) {
        e.preventDefault();
        setDragOver(true);
      }
    },
    onDragLeave: (e: DragEvent) => {
      if (
        e.currentTarget === e.target ||
        !e.currentTarget.contains(e.relatedTarget as Node)
      ) {
        setDragOver(false);
      }
    },
    onDrop: (e: DragEvent) => {
      setDragOver(false);
      if (e.dataTransfer.files.length === 0) return;
      e.preventDefault();
      void storeBrowserFiles(Array.from(e.dataTransfer.files), onAttach, onError);
    },
  };
  return { dragOver, handlers };
}
