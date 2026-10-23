import type { Tab } from "../../lib/types";
import { LockBadge } from "../billing/LockBadge";
import { useTabLock } from "../billing/useTabLock";

/** The lock badge for the tab shown in the tab header. */
export function TabLockMark({ tab }: { tab: Tab | null }) {
  return <LockBadge view={useTabLock(tab)} />;
}
