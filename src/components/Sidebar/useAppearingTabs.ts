import { useEffect, useLayoutEffect, useState } from "react";

const ENTRANCE_MS = 250;
const NONE: ReadonlySet<string> = new Set();
/** Every row this page has shown; a row that comes back is not a new member. */
const shown = new Set<string>();

/** Rows that joined a mounted list. A list mounts with what already exists, so a switch animates nothing. */
export function useAppearingTabs(tabIds: readonly string[]): ReadonlySet<string> {
  const [list, setList] = useState({ ids: tabIds, appearing: NONE });
  let current = list;
  if (list.ids !== tabIds) {
    const joined = tabIds.filter((id) => !shown.has(id));
    current = { ids: tabIds, appearing: joined.length ? new Set([...list.appearing, ...joined]) : list.appearing };
    setList(current);
  }
  useLayoutEffect(() => {
    for (const id of tabIds) shown.add(id);
  }, [tabIds]);
  const { appearing } = current;
  useEffect(() => {
    if (appearing.size === 0) return;
    const timer = setTimeout(() => setList((latest) => ({ ...latest, appearing: NONE })), ENTRANCE_MS);
    return () => clearTimeout(timer);
  }, [appearing]);
  return appearing;
}
