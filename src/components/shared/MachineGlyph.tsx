import { CircleDashed, Cloud, Command, Cpu, Laptop, Monitor, Server, TabletSmartphone } from "lucide-react";
import type { CSSProperties } from "react";
import { machineKind, type MachineIdentity, type MachineKind } from "../../lib/machineIdentity";

const ICONS = {
  laptop: Laptop,
  desktop: Monitor,
  server: Server,
  mac: Command,
  phone: TabletSmartphone,
  vm: Cloud,
  small: Cpu,
  other: CircleDashed,
} satisfies Record<MachineKind, typeof Laptop>;

export function MachineGlyph({ kind, tone, size = "mark-12", label, muted = false }: {
  kind?: string | null;
  tone?: string;
  size?: "mark-12" | "mark-15" | "tile" | "rail";
  label?: string;
  muted?: boolean;
}) {
  const Icon = ICONS[machineKind(kind)];
  const ink = tone ?? (muted ? "var(--ct-muted-fg)" : "var(--ct-accent)");
  const tile = size === "tile" || size === "rail";

  return (
    <span
      aria-hidden={label ? undefined : true}
      aria-label={label}
      role={label ? "img" : undefined}
      className={tile ? "ct-card__icon shrink-0" : "inline-flex shrink-0 items-center align-middle"}
      style={tile ? { "--ct-card-tone": ink, ...(size === "rail" && { width: 36, height: 36 }) } as CSSProperties : { color: ink }}
    >
      <Icon aria-hidden="true" size={size === "mark-12" ? 12 : 15} strokeWidth={size === "mark-12" ? 2.25 : 2} />
    </span>
  );
}

export function MachineIconBadge({ identity }: { identity?: MachineIdentity | null }) {
  return <span className="sidebar-remote-tab-icon__badge [&_svg]:size-2.5"><MachineGlyph {...identity} /></span>;
}
