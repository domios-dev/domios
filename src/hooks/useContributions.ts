/**
 * Generic primitive for plugin contribution lists.
 *
 * Each contribution kind (agent providers, skills, commands, panel renderers,
 * ...) lives in its own backend list. The frontend reads them through this
 * primitive: pass a `kind` and a fetcher, get a cached array that auto-refetches
 * when a `contributions-changed` event with the matching `kind` arrives from
 * the backend.
 *
 * `useAgentProviders()` is a one-line wrapper around this — see ./useAgentProviders.ts.
 * Future contribution hooks (useSkills, useCommands, ...) reuse this primitive
 * unchanged.
 */
import { useCallback, useEffect, useState } from "react";
import { listen, type UnlistenFn } from "../lib/bridge";

export interface ContributionsChangedPayload {
  kind: string;
}

export function useContributions<T>(
  kind: string,
  fetcher: () => Promise<T[]>,
): { items: T[]; loading: boolean; error: string | null; refetch: () => void } {
  const [items, setItems] = useState<T[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    fetcher()
      .then((next) => {
        setItems(next);
        setError(null);
      })
      .catch((e) => {
        setError(String(e));
      })
      .finally(() => setLoading(false));
  }, [fetcher]);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;

    load();

    listen<ContributionsChangedPayload>("contributions-changed", (event) => {
      if (!active) return;
      if (event.payload.kind === kind) {
        load();
      }
    })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {
        // Listener registration failed — items still load via initial fetch.
      });

    return () => {
      active = false;
      if (unlisten) unlisten();
    };
  }, [kind, load]);

  return { items, loading, error, refetch: load };
}
