import { Camera, ChevronLeft, ChevronRight, Crosshair, Globe, Home, RotateCw, Square, X, type LucideIcon } from "lucide-react";

const SURFACE_ICONS: Readonly<Record<string, LucideIcon>> = {
  camera: Camera,
  "chevron-left": ChevronLeft,
  "chevron-right": ChevronRight,
  crosshair: Crosshair,
  globe: Globe,
  home: Home,
  "rotate-cw": RotateCw,
  square: Square,
  x: X,
};

export function surfaceIcon(name: string | undefined): LucideIcon | null {
  return name ? SURFACE_ICONS[name] ?? null : null;
}
