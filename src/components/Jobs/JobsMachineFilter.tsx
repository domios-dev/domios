import type { PairedPeer } from "../../lib/types";
import { DropdownSelect } from "../shared/DropdownMenu";
import { jobsMachineOptions, THIS_MACHINE } from "./jobsMachineModel";
import { useMeshMachineIdentities } from "../../lib/useMeshMachineIdentities";
import { MachineGlyph } from "../shared/MachineGlyph";
import { machineLabel } from "../../lib/machineIdentity";

export function JobsMachineFilter({ value, peers, onChange }: { value: string; peers: readonly PairedPeer[]; onChange: (value: string) => void }) {
  const options = jobsMachineOptions(peers);
  const identities = useMeshMachineIdentities();
  const identityOf = (id: string) => id === THIS_MACHINE ? identities.local_machine_identity : identities.machine_identities?.[id];
  // Content-width beside its label, so the chevron stays with the machine's name instead of the row's far edge.
  return <div className="w-fit min-w-0 max-w-full shrink-0">
    <DropdownSelect selectionIndicator="highlight" aria-label="Machine" prefix="Machine" data-testid="jobs-machine" className="min-w-0 max-w-full" value={value} onChange={onChange} leading={<MachineGlyph {...identityOf(value)} />}
      options={[
        ...(options.some((option) => option.id === value) ? [] : [{ value, label: "Unavailable machine" }]),
        ...options.map((option) => ({
          value: option.id, label: option.name,
          content: <span className="flex min-w-0 items-center gap-1.5" aria-label={machineLabel(option.name, identityOf(option.id), option.online ? "online" : "offline")}>
            <MachineGlyph {...identityOf(option.id)} />
            <span className="min-w-0 flex-1 truncate">{option.name}</span>
            <span className="flex w-12 shrink-0 items-center gap-1 text-[var(--ct-muted-fg)]">
              <span aria-hidden="true" className={`size-1.5 rounded-full ${option.online ? "bg-[var(--ct-green)]" : "bg-[var(--ct-muted)]"}`} />
              {option.online ? "online" : "offline"}
            </span>
          </span>,
        })),
      ]} />
  </div>;
}
