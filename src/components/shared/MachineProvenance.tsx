import { MachineGlyph } from "./MachineGlyph";
import { MACHINE_KINDS, cleanMachineName, machineKind, machineLabel } from "../../lib/machineIdentity";
import { useAccountMachineNames, useMeshMachineIdentity, useMeshPeerName } from "../../lib/useMeshMachineIdentities";

export function MachineProvenance({ peerId, name, status, remote = false }: {
  peerId?: string | null;
  name: string;
  status?: string;
  remote?: boolean;
}) {
  const identity = useMeshMachineIdentity(peerId, true, remote ? name : undefined);
  const accountNames = useAccountMachineNames();
  const displayName = useMeshPeerName(peerId) ?? (!peerId && !remote ? accountNames.local : undefined) ?? cleanMachineName(name, peerId) ?? "Machine";
  const label = machineLabel(displayName, identity, status);
  return (
    <span className="inline-flex min-w-0 max-w-full items-baseline gap-1.5 text-[length:var(--ct-font-size-meta)]" title={label}>
      <MachineGlyph {...identity} />
      <span className="min-w-0 truncate">{displayName}<span className="sr-only">, {MACHINE_KINDS[machineKind(identity?.kind)].word}{status && `, ${status}`}</span></span>
    </span>
  );
}
