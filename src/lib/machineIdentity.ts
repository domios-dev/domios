import { blendColor, hslToRgb, parseHex, rgbToHsl } from "./colors";
import { contrastRatio } from "./themeCss";

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

const presetThemeNames = new Map([
  ["github-dark", "GitHub Dark"],
  ["github-light", "GitHub Light"],
  ["paper-ink", "Paper & Ink"],
]);

export function machineThemeName(name: string): string {
  const preset = presetThemeNames.get(name);
  if (preset) return preset;
  if (!/^[a-z0-9]+(?:[-_][a-z0-9]+)*$/.test(name)) return name;
  return name.split(/[-_]/).map(word => word[0].toUpperCase() + word.slice(1)).join(" ");
}

export function desaturateMachineColor(color: string, online: boolean): string {
  if (online) return color;
  const [hue, saturation, lightness] = rgbToHsl(...parseHex(color));
  const channels = hslToRgb(hue, saturation * 0.35, lightness);
  return `#${channels.map(channel => channel.toString(16).padStart(2, "0")).join("")}`;
}

export function machineGlyphColor(accent: string, background: string, foreground: string, tint = 0): string {
  const contrast = (color: string) => contrastRatio(color, tint ? blendColor(background, color, tint) : background);
  if (contrast(accent) >= 3) return accent;
  const black = "#000000";
  const white = "#ffffff";
  const fallback = contrast(black) > contrast(white) ? black : white;
  const toward = contrast(foreground) >= 3 ? foreground : fallback;
  for (let percent = 5; percent < 100; percent += 5) {
    const color = blendColor(accent, toward, percent / 100);
    if (contrast(color) >= 3) return color;
  }
  return toward;
}

export function machineTileColors(accent: string, background: string, foreground: string, selected: boolean) {
  const tint = selected ? 0.235 : 0.15;
  const tone = machineGlyphColor(accent, background, foreground, tint);
  const fill = blendColor(background, tone, tint);
  return { tone, glyph: machineGlyphColor(tone, fill, foreground) };
}
