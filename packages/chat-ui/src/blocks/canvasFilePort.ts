import { createContext } from "react";

export type CanvasWriteResult = { status: "written" } | { status: "conflict"; current: string };
/** `watching: false` means every live-update slot is taken, so the board shows without live updates. */
export interface CanvasFileWatch { writable: boolean; watching: boolean; release(): void }

/** The host's file-backed canvas operations; absent hosts show file canvases read-only without live reload. */
export interface CanvasFilePort {
  write(path: string, tabId: string, base: string, document: string): Promise<CanvasWriteResult>;
  /** Calls `onChange` when the file changes on disk; `writable` says whether the owner may edit it here. */
  watch(path: string, tabId: string, onChange: () => void): Promise<CanvasFileWatch>;
}

let port: CanvasFilePort | null = null;

export function configureCanvasFilePort(next: CanvasFilePort | null): void {
  port = next;
}

export function canvasFilePort(): CanvasFilePort | null {
  return port;
}

/** The `<ds-canvas src>` file whose items are being read; its items are authorized through it. */
export const CanvasSourceContext = createContext<string | undefined>(undefined);
