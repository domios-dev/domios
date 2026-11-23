import { baseDaemonTransport } from "../daemonBootstrap";
import { createTransportClient, decodeTransportResponse } from "../transportClient";
import { assertWebRequestContext, IS_WEB, relayTarget, webRequestContext } from "../webApi";

const PATH = "/api/live-transcript/capture";

export type LiveCapturePauseReason = "manual" | "focusLost" | "hidden" | "tabChanged" | "microphoneLost" | "failed";
export type LiveCaptureAdmission =
  | { state: "ready"; lease: string }
  | { state: "setup" | "unavailable"; reason: string };

export interface LiveCaptureUpdate {
  lease: string;
  sequence: number;
  provisional: string;
  paused: boolean;
}

export function createLiveTranscriptCaptureTransport(tabId: string) {
  const owner = baseDaemonTransport();
  const token = owner.currentToken();
  const origin = owner.origin();
  const generation = webRequestContext();
  if (!token || relayTarget() || (IS_WEB && (!window.isSecureContext || window.location.origin !== origin))) {
    throw new Error("Live capture needs a direct secure Domios owner window.");
  }
  const client = createTransportClient({ origin, token: () => owner.currentToken() === token ? token : null });
  const assertCurrent = () => {
    assertWebRequestContext(generation);
    if (relayTarget() || baseDaemonTransport() !== owner || owner.currentToken() !== token) {
      throw new DOMException("The live capture connection changed.", "AbortError");
    }
  };

  async function post<T>(path: string, body: string | ArrayBuffer, signal: AbortSignal, pause = false) {
    assertCurrent();
    const response = await client.request!(path, {
      method: "POST",
      body,
      signal,
      redirect: "error",
      keepalive: pause,
      headers: typeof body === "string" ? undefined : { "content-type": "application/octet-stream" },
    }, true);
    const text = await response.text();
    if (!response.ok) throw new Error(text.slice(0, 500) || "The live capture service refused the request.");
    return decodeTransportResponse<T>(response.status, path, text, response.headers.get("content-type"));
  }

  async function update(lease: string, sequence: number, body: string | ArrayBuffer, signal: AbortSignal, paused: boolean) {
    const result = await post<LiveCaptureUpdate>(
      `${PATH}/${lease}/${paused ? "pause" : "frame"}/${sequence}`, body, signal, paused,
    );
    assertCurrent();
    if (result?.lease !== lease || result.sequence !== sequence || result.paused !== paused || typeof result.provisional !== "string") {
      throw new Error("The daemon returned an invalid live capture update.");
    }
    return result;
  }

  return {
    assertCurrent,
    async start(resume: boolean, signal: AbortSignal): Promise<LiveCaptureAdmission> {
      const result = await post<LiveCaptureAdmission>(PATH + "/start", JSON.stringify({ tabId, resume }), signal);
      if (result?.state === "ready" && /^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/.test(result.lease)) return result;
      if ((result?.state === "setup" || result?.state === "unavailable") && typeof result.reason === "string") return result;
      throw new Error("The daemon returned an invalid live capture admission.");
    },
    frame(lease: string, sequence: number, pcm: ArrayBuffer, signal: AbortSignal) {
      return update(lease, sequence, pcm, signal, false);
    },
    pause(lease: string, sequence: number, reason: LiveCapturePauseReason, signal: AbortSignal) {
      return update(lease, sequence, JSON.stringify({ reason }), signal, true);
    },
    async cancel(lease: string) {
      if (owner.currentToken() !== token) return;
      await client.request!(`${PATH}/${lease}/cancel`, {
        method: "POST", body: "{}", redirect: "error", keepalive: true, signal: AbortSignal.timeout(5000),
      }, true);
    },
  };
}
