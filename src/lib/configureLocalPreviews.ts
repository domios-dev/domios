import { nativeBridge } from "./nativeBridge";
import { configureCanvasFilePort, configureHtmlFileLoader, configureLocalArtifactLoader, configureLocalArtifactOpener, type CanvasFilePort, type CanvasWriteResult, type LocalPreviewFile } from "@codeterm/chat-ui";
import { invoke, listen } from "./bridge";
import { recordClientHealth } from "./clientJournal";
import { remoteChatFailure, remoteChatState } from "./remoteChatState";
import { classifyTabTarget } from "./remoteTabTransport";
import { relayTarget } from "./webApi";

const PREVIEW_UNAVAILABLE = "This file cannot be previewed. It may be missing or access is restricted.";
const MACHINE_OFFLINE = "The machine that has this file is offline.";
const MACHINE_ASLEEP = "The machine that has this file is asleep or offline.";
const MACHINE_SIGNED_OUT = "The machine that has this file is signed out.";
const MACHINE_TOO_OLD = "The machine that has this file needs an update to preview it.";

const ASLEEP_CODES = new Set(["machine_sleeping", "credits_exhausted", "machine_stopped"]);
const OFFLINE_CODES = new Set([
  "peer_unroutable",
  "peer_unreachable",
  "peer_name_no_online_record",
  "mesh_transport_nothing_sent",
  "peer_local_api_unreachable",
  "relay_delivery_deadline_exceeded",
]);

export function previewFailureMessage(
  error: unknown,
  tabId?: string,
  hasRelayTarget = relayTarget() !== null,
): string {
  const isRemote = hasRelayTarget || classifyTabTarget(tabId).kind === "remote";
  if (!isRemote) return PREVIEW_UNAVAILABLE;
  const failure = remoteChatFailure(error);
  if (failure.status === 402 || (failure.code !== undefined && ASLEEP_CODES.has(failure.code))
    || /machine.*sleep|asleep|credits_exhausted/i.test(failure.raw)) {
    return MACHINE_ASLEEP;
  }
  if (failure.code !== undefined && OFFLINE_CODES.has(failure.code)) {
    return MACHINE_OFFLINE;
  }
  const state = remoteChatState(undefined, failure);
  if (state === "offline" || state === "unreachable") return MACHINE_OFFLINE;
  if (state === "signed-out") return MACHINE_SIGNED_OUT;
  if (state === "too-old") return MACHINE_TOO_OLD;
  return PREVIEW_UNAVAILABLE;
}

/** Canvas write-back and change events stay on this machine; a peer's tab shows its canvases read-only. */
const canvasFilePort: CanvasFilePort = {
  write: (path, tabId, base, document) => invoke<CanvasWriteResult>("write_local_canvas_file", { path, tabId, base, document }),
  async watch(path, tabId, onChange) {
    if (relayTarget() !== null || classifyTabTarget(tabId).kind !== "local") return { writable: false, watching: false, release() {} };
    const unlisten = await listen<{ path?: string }>(`local-canvas-changed-${tabId}`, (event) => { if (event.payload?.path === path) onChange(); });
    try {
      const { writable, watching } = await invoke<{ writable: boolean; watching: boolean }>("watch_local_canvas_file", { path, tabId, watch: true });
      // A refused watch holds no slot, so releasing it must not free another viewer's.
      if (!watching) unlisten();
      let released = !watching;
      return {
        writable,
        watching,
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

async function measuredPreview<T>(tabId: string | undefined, load: () => Promise<T>): Promise<T> {
  const started = performance.now();
  try {
    const value = await load();
    const elapsedMs = Math.round(performance.now() - started);
    if (elapsedMs >= 2_000) recordClientHealth({ event: "preview-slow", elapsedMs });
    return value;
  } catch (error) {
    recordClientHealth({ event: "preview-failed", elapsedMs: Math.round(performance.now() - started) });
    throw new Error(previewFailureMessage(error, tabId));
  }
}

export function configureLocalPreviews(): void {
  if (!watchingImages && typeof document !== "undefined") {
    watchingImages = true;
    document.addEventListener("error", event => {
      if (event.target instanceof HTMLImageElement) recordClientHealth({ event: "preview-failed", elapsedMs: 0 });
    }, true);
  }
  configureHtmlFileLoader((path, tabId, canvas) => measuredPreview(tabId, () => invoke<string>("read_html_preview_file", { path, tabId, ...(canvas ? { canvas } : {}) })));
  configureLocalArtifactLoader((path, kind, tabId, canvas) => measuredPreview(tabId, () => invoke<LocalPreviewFile>("read_local_preview_file", { path, kind, tabId, ...(canvas ? { canvas } : {}) })));
  configureCanvasFilePort(canvasFilePort);
  configureLocalArtifactOpener(
    nativeBridge.window ? (path) => nativeBridge.revealFile(path) : null,
    (tabId) => Boolean(nativeBridge.window) && relayTarget() === null && classifyTabTarget(tabId).kind === "local",
  );
}
