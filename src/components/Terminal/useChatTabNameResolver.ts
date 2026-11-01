import { useLatestCallback } from "../../hooks/useLatestCallback";

export function useChatTabNameResolver(names: ReadonlyMap<string, string>) {
  return useLatestCallback((ref: string): string | undefined => names.get(ref), [names]);
}
