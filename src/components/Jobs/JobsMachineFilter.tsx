import type { PairedPeer } from "../../lib/types";
import { DropdownSelect } from "../shared/DropdownMenu";
import { jobsMachineOptions, THIS_MACHINE } from "./jobsMachineModel";
import { useAccountMachineNames, useMeshMachineIdentities } from "../../lib/useMeshMachineIdentities";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachinePickerRow, MachineThisBadge, machineRowLabel } from "../shared/MachineRow";

export function JobsMachineFilter({ value, peers, onChange }: { value: string; peers: readonly PairedPeer[]; onChange: (value: string) => void }) {
  const names = useAccountMachineNames();
  const identities = useMeshMachineIdentities();
  const options = jobsMachineOptions(peers, identities.local_instance_name).map(option => ({ ...option, name: (option.id === THIS_MACHINE ? names.local : names.peers.get(option.id)) ?? option.name }));
  const identityOf = (id: string) => id === THIS_MACHINE ? identities.local_machine_identity : identities.machine_identities?.[id];
  // Content-width beside its label, so the chevron stays with the machine's name instead of the row's far edge.
  return <div className="w-fit min-w-0 max-w-full shrink-0">
    <DropdownSelect selectionIndicator="highlight" aria-label="Machine" prefix="Machine" data-testid="jobs-machine" className="min-w-0 max-w-full" value={value} onChange={onChange} leading={<MachineGlyph {...identityOf(value)} />}
      trailing={value === THIS_MACHINE && <MachineThisBadge />}
      options={[
        ...(options.some((option) => option.id === value) ? [] : [{ value, label: "Unavailable machine" }]),
        ...options.map(option => {
          const row = { name: option.name, identity: identityOf(option.id), status: option.online ? "online" : "offline", local: option.id === THIS_MACHINE };
          return { value: option.id, label: option.name, ariaLabel: machineRowLabel(row), stretchLabelContent: true, content: <MachinePickerRow {...row} /> };
        }),
      ]} />
  </div>;
}
