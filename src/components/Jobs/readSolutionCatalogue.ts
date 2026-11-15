import { readWhenMuxCapacityAvailable } from "../../lib/streamMuxRequestModel";
import type { BlueprintSolutionsApi } from "./blueprintSolutions";
import { catalogEntry, type CatalogEntry } from "./solutionCatalog";

export async function readSolutionCatalogue(sources: readonly string[], api: BlueprintSolutionsApi, signal: AbortSignal): Promise<CatalogEntry[]> {
  const results: CatalogEntry[][] = new Array(sources.length);
  const pending = sources.entries();
  const worker = async () => {
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
  };
  await Promise.all(Array.from({ length: Math.min(3, sources.length) }, worker));
  return results.flat();
}
