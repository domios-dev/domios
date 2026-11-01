import { nativeBridge } from "./nativeBridge";
import type { PluginDocument } from "./pluginDocument";
import { viewHostResult, type PickedFile, type ViewHostDeps, type ViewHostMediaRequest, type ViewHostResult } from "./pluginViewHostActions";

// Every host action goes through the asking document's own `hostRequest`: it stamps that
// document's private lease, so the daemon acts only for the exact document that asked, and a
// response for a retired or reminted document is refused.
type HostRequest = PluginDocument["hostRequest"];

const JSON_HEADERS = { "Content-Type": "application/json" };
const STALE: ViewHostResult = { ok: false, code: "denied", reason: "stale" };

/** File-type filters for the native dialog, from the passive media types a view may pick. */
const EXTENSIONS: Record<string, string[]> = {
  "image/png": ["png"], "image/jpeg": ["jpg", "jpeg"], "image/webp": ["webp"], "image/gif": ["gif"],
  "audio/mpeg": ["mp3"], "audio/ogg": ["ogg", "oga"], "audio/wav": ["wav"],
  "video/mp4": ["mp4", "m4v"], "video/webm": ["webm"], "video/ogg": ["ogv"],
};

async function json(hostRequest: HostRequest, route: Parameters<HostRequest>[0], body: unknown): Promise<unknown> {
  try {
    const { response, current } = await hostRequest(route, { body: JSON.stringify(body), headers: JSON_HEADERS });
    const answer: unknown = await response.json().catch(() => null);
    return current() ? answer : STALE;
  } catch {
    return STALE;
  }
}

/** Streams the plugin's file and hands it to the browser as a download, if the frame and its lease are still current. */
function download(hostRequest: HostRequest) {
  return async (request: ViewHostMediaRequest, fileName: string, isCurrent: () => boolean): Promise<ViewHostResult> => {
    let answer;
    try {
      answer = await hostRequest("/api/plugins/view-host-action/download", { body: JSON.stringify(request), headers: JSON_HEADERS });
    } catch {
      return STALE;
    }
    const { response, current } = answer;
    if (!response.ok) {
      const refusal = viewHostResult(await response.json().catch(() => null));
      return refusal.ok ? { ok: false, code: "unavailable", reason: "host" } : refusal;
    }
    const blob = await response.blob();
    if (!current() || !isCurrent()) return STALE;
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
  };
}

/** The browser's own picker, in the app's document so the chosen File never passes through the frame. */
function browserPicker(accept: string[], multiple: boolean): Promise<PickedFile[] | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept.join(",");
    input.multiple = multiple;
    input.addEventListener("change", () => resolve([...(input.files ?? [])].map((file) => ({ kind: "file", file }))), { once: true });
    input.addEventListener("cancel", () => resolve(null), { once: true });
    input.click();
  });
}

function nativePicker(dialogs: NonNullable<typeof nativeBridge.dialogs>) {
  return async (accept: string[], multiple: boolean): Promise<PickedFile[] | null> => {
    const extensions = accept.flatMap((kind) => EXTENSIONS[kind] ?? []);
    const chosen = await dialogs.open({ multiple, directory: false, filters: [{ name: "Media", extensions }] });
    const paths = chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
    return paths.length ? paths.map((path) => ({ kind: "path", path, name: path.split(/[\\/]/).pop() ?? "file" })) : null;
  };
}

/** Sends one picked file to ingestion: the dialog's path as JSON, or the browser File as the body. */
function ingest(hostRequest: HostRequest) {
  return async (pluginId: string, picked: PickedFile): Promise<unknown> => {
    try {
      const { response, current } = picked.kind === "path"
        ? await hostRequest("/api/plugins/view-input", {
          body: JSON.stringify({ pluginId, path: picked.path, fileName: picked.name }),
          headers: JSON_HEADERS,
        })
        : await hostRequest("/api/plugins/view-input", {
          body: picked.file,
          headers: {
            "Content-Type": picked.file.type in EXTENSIONS ? picked.file.type : "application/octet-stream",
            "X-Domios-Plugin-Input-Name": encodeURIComponent(picked.file.name.slice(0, 120)),
            "X-Domios-Plugin-Id": pluginId,
          },
        });
      const answer: unknown = await response.json().catch(() => null);
      return current() ? answer : STALE;
    } catch {
      return STALE;
    }
  };
}

/**
 * The native dialogs and file manager exist only in the desktop app; the web mirror downloads,
 * picks with a browser file input and refuses reveal. Without a document there is nothing to act for.
 */
export function viewHostDeps(hostRequest: HostRequest | undefined): ViewHostDeps {
  const dialogs = nativeBridge.dialogs;
  const clipboard = globalThis.navigator?.clipboard;
  if (!hostRequest) {
    return { call: async () => STALE, download: async () => STALE };
  }
  return {
    call: (request) => json(hostRequest, "/api/plugins/view-host-action", request),
    writeText: clipboard ? (text) => clipboard.writeText(text) : undefined,
    pickDestination: dialogs ? (defaultPath) => dialogs.save({ defaultPath }) : undefined,
    download: download(hostRequest),
    reveal: dialogs ? (path) => nativeBridge.revealFile(path) : undefined,
    pickFiles: dialogs ? nativePicker(dialogs) : browserPicker,
    ingest: ingest(hostRequest),
  };
}
