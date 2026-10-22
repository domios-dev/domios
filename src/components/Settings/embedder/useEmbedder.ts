import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../../lib/bridge";
import {
  parseOption,
  statusFromEvent,
  type EmbedderListing,
  type EmbedderStatus,
  type Refusal,
  type SwitchPreview,
} from "./embedderModel";

export const EMBEDDER_CHANGED_EVENT = "mem-embedder-changed";

async function readJson<T>(response: Response): Promise<T> {
  const body = await response.json();
  if (!response.ok) {
    const refusal: Refusal = body?.error ?? { message: `HTTP ${response.status}`, hints: [] };
    throw refusal;
  }
  return body as T;
}

export function useEmbedder(apiBase: string, apiHeaders: HeadersInit) {
  const [listing, setListing] = useState<EmbedderListing | null>(null);
  const [status, setStatus] = useState<EmbedderStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const headers = useRef(apiHeaders);
  headers.current = apiHeaders;

  const load = useCallback(async () => {
    try {
      const next = await readJson<EmbedderListing>(
        await fetch(`${apiBase}/api/mem/embedder/providers`, { headers: headers.current }),
      );
      setListing(next);
      setStatus(next.status);
      setError(null);
    } catch (reason) {
      setError((reason as Refusal)?.message ?? String(reason));
    }
  }, [apiBase]);

  useEffect(() => {
    void load();
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
  }, [load]);

  const preview = useCallback(
    async (value: string): Promise<SwitchPreview> => {
      const { provider, model } = parseOption(value);
      const query = new URLSearchParams({ provider, model });
      return readJson<SwitchPreview>(
        await fetch(`${apiBase}/api/mem/embedder/preview?${query}`, { headers: headers.current }),
      );
    },
    [apiBase],
  );

  const switchTo = useCallback(
    async (value: string): Promise<SwitchPreview> => {
      const { provider, model } = parseOption(value);
      const result = await readJson<SwitchPreview>(
        await fetch(`${apiBase}/api/mem/embedder/use`, {
          method: "POST",
          headers: { ...headers.current, "Content-Type": "application/json" },
          body: JSON.stringify({ provider, model }),
        }),
      );
      await load();
      return result;
    },
    [apiBase, load],
  );

  return { listing, status, error, preview, switchTo };
}
