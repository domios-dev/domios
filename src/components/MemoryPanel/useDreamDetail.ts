import { useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../lib/types";
import { fetchLabels } from "./memoryApi";

export function useDreamLabels(api: ApiConfig, ids: string[]) {
  const key = JSON.stringify([api.host, api.port, api.token, ids]);
  const [result, setResult] = useState<{ key: string; titles: Record<string, string> }>({ key: "", titles: {} });
  useEffect(() => {
    const abort = new AbortController();
    const selected = (JSON.parse(key) as [string, number, string, string[]])[3];
    void (async () => {
      const titles: Record<string, string> = {};
      for (let start = 0; start < selected.length && !abort.signal.aborted; start += 50) {
        try {
          const rows = await fetchLabels(api, selected.slice(start, start + 50), abort.signal);
          for (const row of rows) titles[row.id] = row.title;
        } catch {
          if (abort.signal.aborted) return;
        }
      }
      if (!abort.signal.aborted) setResult({ key, titles });
    })();
    return () => abort.abort();
  }, [api.host, api.port, api.token, key]);
  return result.key === key ? result.titles : {};
}

export function useDreamLayout() {
  const ref = useRef<HTMLDivElement>(null);
  const [compact, setCompact] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => setCompact(entry.contentRect.width < 760));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return { ref, compact };
}

export function useDreamSheet(open: boolean) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || !open) return;
    dialog.showModal();
    return () => dialog.close();
  }, [open]);
  return ref;
}
