import { CircleDashed, Cloud, Command, Cpu, Laptop, Monitor, Server, TabletSmartphone } from "lucide-react";
import type { CSSProperties } from "react";
import { machineColor, machineKind, type MachineKind } from "../../lib/machineIdentity";

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

export function MachineGlyph({ kind, color, size = "mark-12", label }: {
  kind?: string | null;
  color?: string | null;
  size?: "mark-12" | "mark-15" | "tile" | "rail";
  label?: string;
}) {
  const Icon = ICONS[machineKind(kind)];
  const tone = `var(--ct-machine-${machineColor(color)})`;
  const tile = size === "tile" || size === "rail";

  return (
    <span
      aria-hidden={label ? undefined : true}
      aria-label={label}
      role={label ? "img" : undefined}
      className={tile ? "ct-card__icon shrink-0" : "inline-flex shrink-0 items-center align-middle"}
      style={tile ? { "--ct-card-tone": tone, ...(size === "rail" && { width: 36, height: 36 }) } as CSSProperties : { color: tone }}
    >
      <Icon aria-hidden="true" size={size === "mark-12" ? 12 : 15} strokeWidth={size === "mark-12" ? 2.25 : 2} />
    </span>
  );
}
