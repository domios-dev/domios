import type { NeighborPermission } from "./neighborRuntimeClient";

export type TrustPreset = "trusted" | "acquaintance" | "collaborator" | "custom";

type PresetFields = Pick<NeighborPermission, "allow_memory" | "allow_files" | "allow_tab_requests" | "allow_interactive_content" | "machine_access">;

// Provisional values until the NEIGHBORS doc fixes them; every preset lives only here.
const PRESETS: Record<Exclude<TrustPreset, "custom">, Required<PresetFields>> = {
  trusted: { allow_memory: true, allow_files: true, allow_tab_requests: true, allow_interactive_content: true, machine_access: "read_only" },
  acquaintance: { allow_memory: false, allow_files: false, allow_tab_requests: false, allow_interactive_content: false, machine_access: "read_only" },
  collaborator: { allow_memory: true, allow_files: true, allow_tab_requests: false, allow_interactive_content: false, machine_access: "read_only" },
};

export const TRUST_OPTIONS: { value: TrustPreset; label: string }[] = [
  { value: "trusted", label: "Trusted friend" },
  { value: "acquaintance", label: "Acquaintance" },
  { value: "collaborator", label: "Project collaborator" },
  { value: "custom", label: "Custom" },
];

/** The preset these permissions match exactly; any other combination is Custom. */
export function trustPresetOf(permission: NeighborPermission): TrustPreset {
  const fields: Required<PresetFields> = {
    allow_memory: permission.allow_memory !== false,
    allow_files: permission.allow_files !== false,
    allow_tab_requests: permission.allow_tab_requests,
    allow_interactive_content: permission.allow_interactive_content === true,
    machine_access: permission.machine_access ?? "read_only",
  };
  const match = (Object.keys(PRESETS) as Array<keyof typeof PRESETS>)
    .find(preset => (Object.keys(fields) as Array<keyof PresetFields>).every(key => PRESETS[preset][key] === fields[key]));
  return match ?? "custom";
}

export function trustPresetFields(preset: Exclude<TrustPreset, "custom">): Required<PresetFields> {
  return PRESETS[preset];
}
