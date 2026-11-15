import { readWhenMuxCapacityAvailable } from "../../lib/streamMuxRequestModel";
import type { BlueprintSolutionsApi } from "./blueprintSolutions";
import { catalogEntry, type CatalogEntry } from "./solutionCatalog";

export async function readSolutionCatalogue(sources: readonly string[], api: BlueprintSolutionsApi, caller: AbortSignal): Promise<CatalogEntry[]> {
  const controller = new AbortController();
  const forward = () => controller.abort(caller.reason);
  if (caller.aborted) forward();
  else caller.addEventListener("abort", forward, { once: true });
  const signal = controller.signal;
  const results: CatalogEntry[][] = new Array(sources.length);
  const pending = sources.entries();
  const worker = async () => {
    try {
      for (const [index, workspaceId] of pending) {
        signal.throwIfAborted();
        const solutions = await readWhenMuxCapacityAvailable(() => api.list(workspaceId, signal), signal);
        const entries: CatalogEntry[] = [];
        for (const entry of solutions) {
          signal.throwIfAborted();
          const stored = await readWhenMuxCapacityAvailable(() => api.document(entry.id, signal), signal)
            .catch((failure: unknown) => { signal.throwIfAborted(); return failure instanceof Error ? failure : new Error(String(failure)); });
          entries.push(catalogEntry({ ...entry, workspaceId }, stored));
        }
        results[index] = entries;
      }
    } catch (error) { controller.abort(error); throw error; }
  };
  try {
    await Promise.all(Array.from({ length: Math.min(3, sources.length) }, worker));
    return results.flat();
  } finally { caller.removeEventListener("abort", forward); }
}
