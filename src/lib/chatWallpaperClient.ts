import type { ChatWallpaper } from "../../gen/ChatWallpaper";
import { invoke } from "./bridge";
import { baseDaemonTransport, daemonTransportForPeer } from "./daemonBootstrap";
import { browserFetch } from "./transportClient";
import type { ThemeConfig } from "./types";
import type { AppearanceTuning } from "./appearance";

export const WALLPAPER_FILE_TYPES = ["image/png", "image/jpeg", "image/webp", "image/svg+xml"] as const;
export const WALLPAPER_RASTER_BYTES = 2 * 1024 * 1024;
export const WALLPAPER_SVG_BYTES = 64 * 1024;
const ASSET_BYTES = 16 * 1024 * 1024;
const ASSET_PATH = "/api/chat-wallpaper/assets";
const SETTING_PATH = "/api/chat-wallpaper";
const LOOK_PATH = "/api/chat-wallpaper/appearance";

export interface MachineAppearance {
  theme: ThemeConfig;
  wallpaper: ChatWallpaper | null;
  tuning?: AppearanceTuning | null;
}

function checkedId(value: unknown): string {
  if (typeof value !== "string" || !/^[a-f0-9]{64}\.(png|svg)$/.test(value)) {
    throw new Error("Invalid wallpaper asset");
  }
  return value;
}

async function checked(response: Response): Promise<Response> {
  if (!response.ok) throw new Error((await response.text()).trim() || "Wallpaper operation unavailable");
  return response;
}

export function readMachineAppearance(peerId: string | undefined, signal?: AbortSignal) {
  return daemonTransportForPeer(peerId).json<MachineAppearance>("GET", LOOK_PATH, undefined, { signal });
}

export async function saveWallpaper(peerId: string | undefined, wallpaper: ChatWallpaper | null, expectedWallpaper?: ChatWallpaper) {
  await daemonTransportForPeer(peerId).json("PUT", SETTING_PATH, { wallpaper, expected_wallpaper: expectedWallpaper });
}

export async function readWallpaperAsset(peerId: string | undefined, value: string, signal?: AbortSignal): Promise<Blob> {
  const id = checkedId(value);
  const type = id.endsWith(".svg") ? "image/svg+xml" : "image/png";
  const path = "/api/serve-file?wallpaper=" + encodeURIComponent(id);
  if (!peerId) {
    const response = await checked(await baseDaemonTransport().request!(path, { signal }, true));
    const blob = await response.blob();
    if (blob.type !== type || blob.size > ASSET_BYTES) throw new Error("Invalid wallpaper asset");
    return blob;
  }
  const encoded = await daemonTransportForPeer(peerId).json<{ media_type: string; data: string }>(
    "GET", path + "&encoded=true", undefined, { signal },
  );
  if (encoded.media_type !== type || typeof encoded.data !== "string" || encoded.data.length > Math.ceil(ASSET_BYTES / 3) * 4) {
    throw new Error("Invalid wallpaper asset");
  }
  const blob = await (await browserFetch("data:" + type + ";base64," + encoded.data, { signal })).blob();
  if (blob.size > ASSET_BYTES || blob.type !== type) throw new Error("Invalid wallpaper asset");
  return blob;
}

function inputType(file: File): string {
  if (file.type) return file.type;
  const extension = file.name.split(".").pop()?.toLowerCase();
  switch (extension) {
    case "jpeg":
    case "jpg": return "image/jpeg";
    case "png": return "image/png";
    case "webp": return "image/webp";
    case "svg": return "image/svg+xml";
    default: return "";
  }
}

async function upload(peerId: string | undefined, bytes: Blob, type: string, copyId?: string): Promise<string> {
  const path = peerId ? "/api/mesh/chat-wallpaper/assets/" + encodeURIComponent(peerId)
      + (copyId ? "?copy_id=" + encodeURIComponent(copyId) : "")
    : copyId ? ASSET_PATH + "/copy/" + encodeURIComponent(copyId) : ASSET_PATH;
  const response = await checked(await baseDaemonTransport().request!(path, {
    method: "POST", headers: { "Content-Type": type }, body: bytes,
  }, true));
  if (!peerId && copyId) return copyId;
  const reply = await response.json();
  if (peerId && !reply.ok) {
    const reason = typeof reply.data === "string" ? reply.data : reply.error;
    throw new Error(typeof reason === "string" ? reason : "The target machine refused the upload");
  }
  return checkedId(peerId ? reply.data?.id : reply.id);
}

export async function uploadWallpaper(peerId: string | undefined, file: File): Promise<ChatWallpaper> {
  const type = inputType(file);
  if (!(WALLPAPER_FILE_TYPES as readonly string[]).includes(type)) {
    throw new Error("This file type is unsupported. Choose a PNG, JPEG, WebP, or SVG file.");
  }
  const svg = type === "image/svg+xml";
  if (file.size > (svg ? WALLPAPER_SVG_BYTES : WALLPAPER_RASTER_BYTES)) {
    throw new Error(svg
      ? "The SVG is too large. Choose an SVG up to 64 KB."
      : "The image is too large. Choose an image up to 2 MB.");
  }
  const id = await upload(peerId, file, type);
  const label = file.name.replace(/[\u0000-\u001f\u007f/\\]/g, "").trim();
  return {
    background: { pattern: svg ? "svg" : "image" },
    asset: id,
    asset_label: new TextEncoder().encode(label).length <= 255 && label ? label : undefined,
  };
}

export async function copyWallpaperAsset(peerId: string | undefined, id: string, asset: Blob): Promise<void> {
  if (await upload(peerId, asset, asset.type, checkedId(id)) !== id) throw new Error("Wallpaper copy did not match");
}

export async function deleteWallpaperAsset(peerId: string | undefined, id: string): Promise<void> {
  await checked(await daemonTransportForPeer(peerId).request!(
    ASSET_PATH + "/" + encodeURIComponent(checkedId(id)), { method: "DELETE" },
  ));
}

export async function askGeneralForWallpaper(peerId: string | undefined, requestText: string): Promise<void> {
  const result = await daemonTransportForPeer(peerId).json<{ request: string; tab_id: string }>("POST", SETTING_PATH + "/drafts");
  if (!/^[a-f0-9-]{36}$/i.test(result.request) || !result.tab_id) throw new Error("General did not accept the request");
  const text = [
    "The owner asked you to draw a wallpaper for this machine.",
    "Read the chat background section of domios docs RUNTIME-CONFIG. Draw one seamless square SVG tile with a viewBox, static line art, fill=\"none\", and stroke=\"currentColor\". Keep it under 64 KiB and save it as wallpaper.svg in your current pad.",
    "Apply it by running: domios wallpaper set --request '" + result.request + "' --from-pad wallpaper.svg",
    "Artwork description: " + requestText,
  ].join("\n\n");
  const tabId = peerId ? "mesh:" + peerId + "/" + result.tab_id : result.tab_id;
  await invoke("chat_send_text", { tabId, text });
}
