import { invoke } from "../../../lib/bridge";
import { IS_WEB, webUpload } from "../../../lib/webApi";

// A near-instant tap (e.g. finger lifted while granting the mic permission) yields a container-header-only clip
// — non-zero but a few KB with no real audio. Discard anything under this floor so empty voice notes never send.
export const MIN_VOICE_BYTES = 5 * 1024;

export function pickAudioMime(): string | undefined {
  if (typeof MediaRecorder === "undefined") return undefined;
  // iOS Safari records mp4/aac; everything else does webm/opus.
  return ["audio/ogg;codecs=opus", "audio/ogg", "audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find((m) =>
    MediaRecorder.isTypeSupported(m),
  );
}

export function audioExt(mime: string): string {
  if (mime.includes("ogg")) return "ogg";
  if (mime.includes("mp4")) return "m4a";
  return "webm";
}

export function microphoneErrorMessage(error: unknown): string {
  if (error instanceof DOMException) {
    if (error.name === "NotAllowedError" || error.name === "SecurityError") {
      return "Microphone permission is blocked for CodeTerm.";
    }
    if (error.name === "NotFoundError") {
      return "No microphone was found for CodeTerm.";
    }
  }
  return error instanceof Error ? error.message : String(error);
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsDataURL(blob);
  });
}

/** Persist a browser blob where the agent can read it: daemon upload in
 * web mode, local temp file via Tauri in desktop mode. */
export async function storeBlob(blob: Blob, filename: string): Promise<string> {
  if (IS_WEB) {
    return (await webUpload(blob, filename)).path;
  }
  const ext = filename.includes(".") ? filename.split(".").pop()! : "bin";
  return invoke<string>("save_temp_file", {
    data: await blobToBase64(blob),
    extension: ext,
    name: filename,
  });
}
