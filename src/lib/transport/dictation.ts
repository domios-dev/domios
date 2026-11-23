import { baseDaemonTransport } from "../daemonBootstrap";
import { assertWebRequestContext, IS_WEB, relayTarget, webRequestContext } from "../webApi";
import { createTransportClient, decodeTransportResponse } from "../transportClient";
import type { DictationStartResult } from "../../../gen/DictationStartResult";
import type { DictationUpdate } from "../../../gen/DictationUpdate";

export const DICTATION_PCM_BYTES = 16_000;
export const DICTATION_LIMIT_MS = 300_000;

export function createDictationTransport(session: string, tabId: string) {
  const owner = baseDaemonTransport();
  const generation = webRequestContext();
  const token = owner.currentToken();
  const origin = owner.origin();
  if (!token || relayTarget() || (IS_WEB && (!window.isSecureContext || window.location.origin !== origin))) {
    throw new Error("Live dictation needs a direct secure (HTTPS) Domios owner window.");
  }
  // Keep redirect and unload semantics intact by using the direct daemon client.
  const client = createTransportClient({ origin, token: () => owner.currentToken() === token ? token : null });
  const path = `/api/dictation/${encodeURIComponent(session)}`;
  const assertCurrent = () => {
    assertWebRequestContext(generation);
    if (relayTarget() || baseDaemonTransport() !== owner || owner.currentToken() !== token) {
      throw new DOMException("Dictation connection changed", "AbortError");
    }
  };
  async function post<T>(endpoint: string, body: string | ArrayBuffer, signal: AbortSignal, binary = false): Promise<T> {
    assertCurrent();
    const res = await client.request!(endpoint, {
      method: "POST", body, signal, redirect: "error",
      headers: binary ? { "content-type": "application/octet-stream" } : undefined,
    }, true);
    const text = await res.text();
    assertCurrent();
    if (!res.ok) throw new Error(text.slice(0, 500) || "The dictation service did not accept the request.");
    return decodeTransportResponse<T>(res.status, endpoint, text, res.headers.get("content-type"));
  }
  async function update(sequence: number, body: string | ArrayBuffer, final: boolean, signal: AbortSignal) {
    const endpoint = `${path}/${final ? "finish" : "frame"}/${sequence}`;
    const result = await post<DictationUpdate>(endpoint, body, signal, !final);
    if (!result || result.session !== session || result.sequence !== sequence || result.final !== final
      || typeof result.committed !== "string" || typeof result.provisional !== "string") {
      throw new Error("The daemon returned an invalid dictation update.");
    }
    return result;
  }
  return {
    async start() {
      return await post<DictationStartResult>("/api/dictation/start", JSON.stringify({ session, tabId }), AbortSignal.timeout(20_000));
    },
    frame(sequence: number, audio: ArrayBuffer, signal: AbortSignal) {
      return update(sequence, audio, false, signal);
    },
    finish(sequence: number, signal: AbortSignal) {
      return update(sequence, "{}", true, signal);
    },
    async cancel() {
      if (owner.currentToken() !== token) return;
      await client.request!(`${path}/cancel`, {
        method: "POST", body: "{}", keepalive: true, signal: AbortSignal.timeout(5000), redirect: "error",
      }, true);
    },
  };
}
