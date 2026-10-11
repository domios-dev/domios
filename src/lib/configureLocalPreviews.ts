import { nativeBridge } from "./nativeBridge";
import { configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener } from "@codeterm/chat-ui";
import { invoke } from "./bridge";

export function configureLocalPreviews(): void {
  configureHtmlFileLoader((path, tabId) => invoke<string>("read_html_preview_file", { path, tabId }));
  configureLocalArtifactLoader((path, kind, tabId) => invoke("read_local_preview_file", { path, kind, tabId }));
  configureLocalArtifactOpener(nativeBridge.window ? (path) => nativeBridge.revealFile(path) : null);
}
