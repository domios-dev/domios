export type BlueprintExpansion = ReadonlySet<string>;
export const blueprintNodeKey = (path: readonly string[], id: string) => JSON.stringify([...path, id]);
export function toggleBlueprintExpansion(expanded: BlueprintExpansion, key: string): BlueprintExpansion {
  const next = new Set(expanded);
  if (!next.delete(key)) next.add(key);
  return next;
}
