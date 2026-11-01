import type { MachineAccess, NeighborPermission } from "./neighborRuntimeClient";

export type TrustPreset = "trusted" | "acquaintance" | "collaborator" | "custom";
type NamedPreset = Exclude<TrustPreset, "custom">;

interface PresetValues {
  allow_memory: boolean;
  /** True when the preset shares every memory area; false leaves the owner's chosen areas alone. */
  all_memory: boolean;
  allow_files: boolean;
  allow_tab_requests: boolean;
  allow_interactive_content: boolean;
  machine_access: MachineAccess;
  daily_limit: number;
}

const PRESETS: Record<NamedPreset, PresetValues> = {
  trusted: { allow_memory: true, all_memory: true, allow_files: true, allow_tab_requests: true, allow_interactive_content: false, machine_access: "read_only", daily_limit: 100 },
  acquaintance: { allow_memory: false, all_memory: false, allow_files: true, allow_tab_requests: false, allow_interactive_content: false, machine_access: "read_only", daily_limit: 20 },
  collaborator: { allow_memory: true, all_memory: false, allow_files: true, allow_tab_requests: true, allow_interactive_content: false, machine_access: "folder", daily_limit: 50 },
};

export const TRUST_OPTIONS: { value: TrustPreset; label: string }[] = [
  { value: "trusted", label: "Trusted friend" },
  { value: "acquaintance", label: "Acquaintance" },
  { value: "collaborator", label: "Project collaborator" },
  { value: "custom", label: "Custom" },
];

/** The preset these permissions and this daily limit match exactly; anything else, including server defaults, is Custom. */
export function trustPresetOf(permission: NeighborPermission, dailyLimit: number | null): TrustPreset {
  const memory = permission.allow_memory !== false;
  const match = (Object.keys(PRESETS) as NamedPreset[]).find(name => {
    const preset = PRESETS[name];
    return preset.allow_memory === memory
      && (!preset.all_memory || !(permission.memory_areas?.length))
      && preset.allow_files === (permission.allow_files !== false)
      && preset.allow_tab_requests === permission.allow_tab_requests
      && preset.allow_interactive_content === (permission.allow_interactive_content === true)
      && preset.machine_access === (permission.machine_access ?? "read_only")
      && preset.daily_limit === dailyLimit;
  });
  return match ?? "custom";
}

/** What applying a preset changes; folder access waits for a folder, so without one it stays read only for now. */
export function applyTrustPreset(name: NamedPreset, permission: NeighborPermission): { patch: Partial<NeighborPermission>; dailyLimit: number; needsFolder: boolean } {
  const preset = PRESETS[name];
  const needsFolder = preset.machine_access === "folder" && !permission.folder;
  return {
    patch: {
      allow_memory: preset.allow_memory,
      ...(preset.all_memory ? { memory_areas: null } : {}),
      allow_files: preset.allow_files,
      allow_tab_requests: preset.allow_tab_requests,
      allow_interactive_content: preset.allow_interactive_content,
      machine_access: needsFolder ? "read_only" : preset.machine_access,
    },
    dailyLimit: preset.daily_limit,
    needsFolder,
  };
}
