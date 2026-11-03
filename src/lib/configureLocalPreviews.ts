import { nativeBridge } from "./nativeBridge";
import { configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener, type LocalPreviewFile } from "@codeterm/chat-ui";
import { invoke } from "./bridge";
import { recordClientHealth } from "./clientJournal";

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
  configureHtmlFileLoader((path, tabId) => measuredPreview(() => invoke<string>("read_html_preview_file", { path, tabId })));
  configureLocalArtifactLoader((path, kind, tabId) => measuredPreview(() => invoke<LocalPreviewFile>("read_local_preview_file", { path, kind, tabId })));
  configureLocalArtifactOpener(nativeBridge.window ? (path) => nativeBridge.revealFile(path) : null);
}
