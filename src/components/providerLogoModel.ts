export type ProviderLogoState = "available" | "loading" | "failed" | "absent";

export function providerLogoPresentation(state: ProviderLogoState): "logo" | "placeholder" {
  return state === "available" ? "logo" : "placeholder";
}

export function initialProviderLogoState(iconSvg?: string | null): ProviderLogoState {
  if (!iconSvg?.trim()) return "absent";
  if (!/<svg\b/i.test(iconSvg)) return "failed";
  return /<image\b/i.test(iconSvg) ? "loading" : "available";
}
