import { MachineGlyph } from "./MachineGlyph";
import { MACHINE_KINDS, machineKind, machineLabel } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";

export function MachineProvenance({ peerId, name, status, hideName = false, remote = false }: {
  peerId?: string | null;
  name: string;
  status?: string;
  hideName?: boolean;
  remote?: boolean;
}) {
  const identity = useMeshMachineIdentity(peerId, true, remote ? name : undefined);
  const label = machineLabel(name, identity, status);
  return (
    <span className="inline-flex min-w-0 items-baseline gap-1.5 text-[length:var(--ct-font-size-meta)]" title={label}>
      <MachineGlyph {...identity} label={hideName ? `on ${label}` : undefined} />
      {!hideName && <span className="min-w-0 truncate">{name}<span className="sr-only">, {MACHINE_KINDS[machineKind(identity?.kind)].word}{status && `, ${status}`}</span></span>}
    </span>
  );
}
