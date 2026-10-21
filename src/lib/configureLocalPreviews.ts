import { nativeBridge } from "./nativeBridge";
import { configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener, type LocalPreviewFile } from "@codeterm/chat-ui";
import { invoke } from "./bridge";

function previewUnavailable(): never {
  throw new Error("This file cannot be previewed. It may be missing or access is restricted.");
}

export function configureLocalPreviews(): void {
  configureHtmlFileLoader((path, tabId) => invoke<string>("read_html_preview_file", { path, tabId }).catch(previewUnavailable));
  configureLocalArtifactLoader((path, kind, tabId) => invoke<LocalPreviewFile>("read_local_preview_file", { path, kind, tabId }).catch(previewUnavailable));
  configureLocalArtifactOpener(nativeBridge.window ? (path) => nativeBridge.revealFile(path) : null);
}
