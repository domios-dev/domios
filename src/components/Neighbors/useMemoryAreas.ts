import { useEffect, useState } from "react";
import { localFetch } from "../../lib/webApi";

/** The memory areas the owner can share by name, read from the memory taxonomy; "(none)" is not an area. */
export function useMemoryAreas(active: boolean): string[] | null {
  const [areas, setAreas] = useState<string[] | null>(null);
  useEffect(() => {
    if (!active) return;
    const controller = new AbortController();
    localFetch("/api/mem/taxonomy", { signal: controller.signal })
      .then(response => response.ok ? response.json() as Promise<unknown> : null)
      .then(value => {
        if (controller.signal.aborted || typeof value !== "object" || value === null) return;
        setAreas(Object.keys(value).filter(area => area && area !== "(none)").sort((a, b) => a.localeCompare(b)));
      }, () => {});
    return () => controller.abort();
  }, [active]);
  return areas;
}
