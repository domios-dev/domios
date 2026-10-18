import type { PeerInventory } from "./peerInventoryModel";
import type { Service } from "./types";
import { UI_BUTTON, UI_TEXT } from "../shared/design";

export function PeerInventorySections({ inventory, onSelect, compact = false }: {
  inventory: PeerInventory;
  onSelect?: (service: Service) => void;
  compact?: boolean;
}) {
  return <div className="flex min-w-0 flex-col gap-3">
    {([ ["Services", inventory.services], ["Crons", inventory.crons] ] as const).map(([label, rows]) => (
      <section key={label} aria-label={label} className="min-w-0">
        <h3 className={`${UI_TEXT.meta} mb-1 flex items-center gap-2 text-[var(--ct-fg)]`}>
          {label}<span className={UI_TEXT.metaMuted}>{rows.length}</span>
        </h3>
        {inventory.message && <p className={UI_TEXT.metaMuted} role="status">{inventory.message}{rows.length > 0 ? " · last known" : ""}</p>}
        {rows.length === 0 && inventory.state === "ready" && <p className={UI_TEXT.metaMuted}>No {label.toLowerCase()}</p>}
        <ul aria-label={label} className={`ct-rows ${compact ? "max-h-24 overflow-y-auto" : ""}`}>
          {rows.map((row) => <li key={row.resourceKey} className="min-w-0">
            {onSelect ? <button type="button" onClick={() => onSelect(row.resource)} className={`${UI_BUTTON.ghost} w-full !justify-start !h-auto min-w-0 text-left`}>
              <span className="min-w-0 flex-1 break-words">{row.resource.name}</span>
              <span className={`${UI_TEXT.metaMuted} shrink-0`}>{row.stateLabel}</span>
            </button> : <div className={`flex min-w-0 items-center gap-2 ${UI_TEXT.meta}`}>
              <span className="min-w-0 flex-1 truncate" title={row.resource.name}>{row.resource.name}</span>
              <span className={`${UI_TEXT.metaMuted} shrink-0`}>{row.stateLabel}</span>
            </div>}
          </li>)}
        </ul>
      </section>
    ))}
  </div>;
}
