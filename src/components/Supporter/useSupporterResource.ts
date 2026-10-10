import { useMemo, useSyncExternalStore } from "react";
import type { ApiConfig } from "../../lib/types";
import { supporterResource } from "./supporterResource";
export { SUPPORTER_JOURNAL_EVENT } from "./supporterResource";

export function useSupporterResource<T>(api: ApiConfig | undefined, path: string) {
  const store = useMemo(() => supporterResource(api, path), [api, path]);
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return { data: snapshot.data as T | null, failed: snapshot.failed };
}
