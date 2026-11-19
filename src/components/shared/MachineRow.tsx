import { machineLabel, type MachineIdentity } from "../../lib/machineIdentity";
import { MachineGlyph } from "./MachineGlyph";

interface MachineRowProps {
  name: string;
  identity?: MachineIdentity;
  status: string;
  local?: boolean;
}

export function machineRowLabel({ name, identity, status, local }: MachineRowProps) {
  return machineLabel(name, identity, local ? "this machine" : status);
}

export function MachineThisBadge({ className = "" }: { className?: string }) {
  return <span className={`ct-neutral-badge shrink-0 px-1 py-px text-[length:var(--ct-font-size-meta)] leading-none ${className}`}>
    this
  </span>;
}

export function MachineStatus({ status }: Pick<MachineRowProps, "status">) {
  const tone = status === "online" ? "var(--ct-green)"
    : status === "update required" || status === "needs update" ? "var(--ct-yellow)" : "var(--ct-muted)";
  return <span className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[length:var(--ct-font-size-meta)] text-[var(--ct-muted-fg)]">
    <span aria-hidden="true" className="size-1.5 shrink-0 rounded-full" style={{ backgroundColor: tone }} />
    {status}
  </span>;
}

export function MachinePickerRow({ name, identity, status, local }: MachineRowProps) {
  return <span className="flex min-w-0 flex-1 items-center gap-2">
    <MachineGlyph {...identity} size="mark-15" />
    <span className="min-w-0 flex-1 truncate">{name}</span>
    <span className="flex w-24 shrink-0 items-center justify-end">
      {local ? <MachineThisBadge /> : <MachineStatus status={status} />}
    </span>
  </span>;
}
