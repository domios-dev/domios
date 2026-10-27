import type { FreeLimits } from "./freeLimitModel";
import { LimitAmount } from "./LimitAmount";

export function AccountLimits({ limits }: { limits: FreeLimits | null }) {
  if (!limits) return null;
  const entries: [string, number | null | undefined][] = [
    ["Workspaces", limits.max.workspaces], ["Teams", limits.max.teams],
    ["Concurrent tabs", limits.max["agent.tabs"]], ["Mesh machines", limits.meshMax], ["Memory items", limits.memoryMax],
  ];
  return <dl aria-label="Your plan limits">{entries.filter(([, max]) => max !== undefined).map(([name, max]) =>
    <div key={name}><dt>{name}</dt><dd><LimitAmount amount={max ?? null} /></dd></div>)}</dl>;
}
