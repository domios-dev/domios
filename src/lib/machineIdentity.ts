export const MACHINE_KINDS = {
  laptop: { label: "Laptop", word: "laptop" },
  desktop: { label: "Desktop PC", word: "desktop PC" },
  server: { label: "Server", word: "server" },
  mac: { label: "Mac", word: "Mac" },
  phone: { label: "Phone or tablet", word: "phone or tablet" },
  vm: { label: "Virtual machine", word: "virtual machine" },
  small: { label: "Small device", word: "small device" },
  other: { label: "Other", word: "machine" },
} as const;

export const MACHINE_COLORS = ["blue", "teal", "green", "yellow", "pink", "violet", "graphite"] as const;
export type MachineKind = keyof typeof MACHINE_KINDS;
export type MachineColor = (typeof MACHINE_COLORS)[number];
export interface MachineIdentity {
  kind: MachineKind;
  color: MachineColor;
}
export type MeshIdentityProjection = {
  local_instance_name?: string;
  local_machine_identity?: MachineIdentity;
  machine_identities?: Record<string, MachineIdentity>;
};

export const UNKNOWN_MACHINE: MachineIdentity = { kind: "other", color: "graphite" };

export function machineKind(kind?: string | null): MachineKind {
  return kind && Object.prototype.hasOwnProperty.call(MACHINE_KINDS, kind) ? kind as MachineKind : "other";
}

export function machineColor(color?: string | null): MachineColor {
  return MACHINE_COLORS.find(candidate => candidate === color) ?? "graphite";
}

export function cleanMachineName(name?: string | null, peerId?: string | null): string | undefined {
  const clean = name?.replace(/\s*\[[^\]]*\]/g, "").trim();
  return clean && clean !== peerId && !/^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i.test(clean)
    ? clean
    : undefined;
}

export function machineLabel(name: string, identity?: MachineIdentity | null, status?: string): string {
  return [cleanMachineName(name) ?? "Machine", MACHINE_KINDS[machineKind(identity?.kind)].word, status].filter(Boolean).join(", ");
}
