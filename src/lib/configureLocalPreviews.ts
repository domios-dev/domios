import { nativeBridge } from "./nativeBridge";
import { configureCanvasFilePort, configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener, type CanvasFilePort, type CanvasWriteResult, type LocalPreviewFile } from "@codeterm/chat-ui";
import { invoke, listen } from "./bridge";
import { recordClientHealth } from "./clientJournal";
import { classifyTabTarget } from "./remoteTabTransport";

/** Canvas write-back and change events stay on this machine; a peer's tab shows its canvases read-only. */
const canvasFilePort: CanvasFilePort = {
  write: (path, tabId, base, document) => invoke<CanvasWriteResult>("write_local_canvas_file", { path, tabId, base, document }),
  async watch(path, tabId, onChange) {
    if (classifyTabTarget(tabId).kind !== "local") return { writable: false, release() {} };
    const unlisten = await listen<{ path?: string }>(`local-canvas-changed-${tabId}`, (event) => { if (event.payload?.path === path) onChange(); });
    try {
      const { writable } = await invoke<{ writable: boolean }>("watch_local_canvas_file", { path, tabId, watch: true });
      let released = false;
      return {
        writable,
        release() {
          if (released) return;
          released = true;
          unlisten();
          void invoke("watch_local_canvas_file", { path, tabId, watch: false }).catch(() => {});
        },
      };
    } catch (error) {
      unlisten();
      throw error;
    }
  },
};

let watchingImages = false;

async function measuredPreview<T>(load: () => Promise<T>): Promise<T> {
  const started = performance.now();
  try {
    const value = await load();
    const elapsedMs = Math.round(performance.now() - started);
    if (elapsedMs >= 2_000) recordClientHealth({ event: "preview-slow", elapsedMs });
    return value;
  } catch {
    recordClientHealth({ event: "preview-failed", elapsedMs: Math.round(performance.now() - started) });
    return previewUnavailable();
  }
}

function previewUnavailable(): never {
  throw new Error("This file cannot be previewed. It may be missing or access is restricted.");
}

export function configureLocalPreviews(): void {
  if (!watchingImages && typeof document !== "undefined") {
    watchingImages = true;
    document.addEventListener("error", event => {
      if (event.target instanceof HTMLImageElement) recordClientHealth({ event: "preview-failed", elapsedMs: 0 });
    }, true);
  }
  configureHtmlFileLoader((path, tabId, canvas) => measuredPreview(() => invoke<string>("read_html_preview_file", { path, tabId, ...(canvas ? { canvas } : {}) })));
  configureLocalArtifactLoader((path, kind, tabId, canvas) => measuredPreview(() => invoke<LocalPreviewFile>("read_local_preview_file", { path, kind, tabId, ...(canvas ? { canvas } : {}) })));
  configureCanvasFilePort(canvasFilePort);
  configureLocalArtifactOpener(nativeBridge.window ? (path) => nativeBridge.revealFile(path) : null);
}
