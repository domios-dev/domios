import { daemonTransportForPeer } from "../../../lib/daemonBootstrap";
import { localFetch } from "../../../lib/webApi";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../../lib/bridge";
import {
  normalizeRefusal,
  parseOption,
  statusFromEvent,
  type EmbedderListing,
  type EmbedderStatus,
  type SwitchPreview,
} from "./embedderModel";

export const EMBEDDER_CHANGED_EVENT = "mem-embedder-changed";

async function readJson<T>(response: Response): Promise<T> {
  if (response.ok) return response.json();
  const body: unknown = await response.json();
  const reason = body !== null && typeof body === "object" && "error" in body ? body.error : body;
  throw normalizeRefusal(reason ?? `HTTP ${response.status}`);
}

export function useEmbedder(apiBase: string, apiHeaders: HeadersInit, peerId?: string) {
  const [listing, setListing] = useState<EmbedderListing | null>(null);
  const [status, setStatus] = useState<EmbedderStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const headers = useRef(apiHeaders);
  headers.current = apiHeaders;

  const request = useCallback(
    (path: string, init: RequestInit = {}) =>
      peerId
        ? daemonTransportForPeer(peerId).request!(path, init)
        : localFetch(path, { ...init, headers: { ...headers.current, ...init.headers } }),
    [peerId],
  );

  const load = useCallback(async () => {
    try {
      const next = await readJson<EmbedderListing>(
        await request(`/api/mem/embedder/providers`),
      );
      setListing(next);
      setStatus(next.status);
      setError(null);
    } catch (reason) {
      setError(normalizeRefusal(reason).message);
    }
  }, [apiBase, request]);

  useEffect(() => {
    setListing(null);
    setStatus(null);
    setError(null);
    void load();
    if (peerId) return;
    let active = true;
    let stop: (() => void) | undefined;
    void listen(EMBEDDER_CHANGED_EVENT, (event: { payload: unknown }) => {
      const next = statusFromEvent(event?.payload);
      if (next) setStatus(next);
    })
      .then((off) => {
        if (active) stop = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      active = false;
      stop?.();
    };
  }, [load, peerId]);

  const preview = useCallback(
    async (value: string): Promise<SwitchPreview> => {
      const { provider, model } = parseOption(value);
      const query = new URLSearchParams({ provider, model });
      return readJson<SwitchPreview>(
        await request(`/api/mem/embedder/preview?${query}`),
      );
    },
    [apiBase, request],
  );

  const switchTo = useCallback(
    async (value: string): Promise<SwitchPreview> => {
      const { provider, model } = parseOption(value);
      const result = await readJson<SwitchPreview>(
        await request(`/api/mem/embedder/use`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ provider, model }),
        }),
      );
      await load();
      return result;
    },
    [apiBase, load, request],
  );

  return { listing, status, error, preview, switchTo };
}
