import type { PeerInventory } from "./peerInventoryModel";
import { peerCapsuleModel } from "./peerCapsuleModel";

const CAPSULE = "inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full bg-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] px-2 text-[11px]";

export function PeerCapsuleSections({ inventory }: { inventory: PeerInventory }) {
  return <div className="flex min-w-0 flex-col gap-2">
    {([["Services", inventory.services], ["Crons", inventory.crons]] as const).map(([label, resources]) => {
      const model = peerCapsuleModel(resources, inventory.state);
      return <section key={label} aria-label={label}>
        <h3 className="mb-1 text-[10px] text-[var(--ct-muted)]">{label}</h3>
        <ul aria-label={label} className="flex flex-col gap-1">
          {model.lines.map((line, index) => <li key={index} className="flex gap-1">
            {line.map(({ row, width, color, muted }) => <span key={row.resourceKey} role="img"
              aria-label={`${row.resource.name}: ${row.stateLabel}`} title={row.resource.name}
              className={`${CAPSULE} ${muted ? "opacity-50" : ""}`} style={{ width, color }}>
              <span aria-hidden="true" className="h-[7px] w-[7px] shrink-0 rounded-full" style={{ background: color }} />
              <span className="truncate">{row.resource.name}</span>
            </span>)}
            {model.more > 0 && index === model.lines.length - 1 && <span className={`${CAPSULE} text-[var(--ct-muted)]`} style={{ width: model.moreWidth }}>+{model.more} more</span>}
          </li>)}
        </ul>
      </section>;
    })}
  </div>;
}
