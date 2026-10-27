import type { FreeLimits } from "./freeLimitModel";
import type { AccountUsage } from "./useAccountUsage";
import { LimitAmount } from "./LimitAmount";

export function AccountLimits({ limits, usage = {} }: { limits: FreeLimits | null; usage?: AccountUsage }) {
  if (!limits) return null;
  const entries: [string, keyof AccountUsage, number | null | undefined][] = [
    ["Workspaces", "workspaces", limits.max.workspaces], ["Teams", "teams", limits.max.teams],
    ["Concurrent tabs", "agent.tabs", limits.max["agent.tabs"]], ["Mesh machines", "mesh.machines", limits.meshMax],
    ["Memory items", "mem.items", limits.memoryMax],
  ];
  return <dl aria-label="Your plan limits" className="m-0 grid w-full grid-cols-[max-content_max-content_minmax(0,1fr)] items-baseline gap-x-6 gap-y-2 text-xs">
    {entries.filter(([, , max]) => max !== undefined).map(([name, key, max]) => {
      const used = usage[key];
      return <div key={key} className="col-span-3 grid grid-cols-subgrid items-baseline">
        <dt className="text-[var(--ct-muted-fg)]">{name}</dt>
        <dd className="col-span-2 m-0 grid grid-cols-subgrid items-baseline whitespace-nowrap tabular-nums">
          <span className="text-right text-[var(--ct-fg)]">{used !== undefined && <>{used} / </>}<LimitAmount amount={max ?? null} /></span>
          <span className="text-[var(--ct-muted-fg)]">{used !== undefined && max != null ? ` (${Math.max(0, max - used)} left)` : null}</span>
        </dd>
      </div>;
    })}
  </dl>;
}
