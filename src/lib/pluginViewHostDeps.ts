import { baseDaemonTransport } from "./daemonBootstrap";
import { nativeBridge } from "./nativeBridge";
import { viewHostResult, type ViewHostDeps, type ViewHostMediaRequest, type ViewHostResult } from "./pluginViewHostActions";

// Plugin views load from this machine's daemon (`usePluginDocument`), so their host actions go
// there too, never through a selected mesh relay: the frame token and media belong to it.
const HOST_ACTION_PATH = "/api/plugins/view-host-action";

/** Streams the plugin's file from the daemon and hands it to the browser as a download, if the frame is still current. */
async function download(request: ViewHostMediaRequest, fileName: string, isCurrent: () => boolean): Promise<ViewHostResult> {
  const response = await baseDaemonTransport().request!(`${HOST_ACTION_PATH}/download`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(request),
  }, true);
  if (!response.ok) return viewHostResult(await response.json().catch(() => null));
  const blob = await response.blob();
  if (!isCurrent()) return { ok: false, code: "denied", reason: "stale" };
  const url = URL.createObjectURL(blob);
  try {
    const link = document.createElement("a");
    link.href = url;
    link.download = fileName;
    link.rel = "noopener";
    link.click();
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
  return { ok: true };
}

/** The native dialog and file manager exist only in the desktop app; the web mirror downloads and refuses reveal. */
export function viewHostDeps(): ViewHostDeps {
  const dialogs = nativeBridge.dialogs;
  const clipboard = globalThis.navigator?.clipboard;
  return {
    call: (request) => baseDaemonTransport().json("POST", HOST_ACTION_PATH, request),
    writeText: clipboard ? (text) => clipboard.writeText(text) : undefined,
    pickDestination: dialogs ? (defaultPath) => dialogs.save({ defaultPath }) : undefined,
    download,
    reveal: dialogs ? (path) => nativeBridge.revealFile(path) : undefined,
  };
}
