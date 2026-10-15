import { apiHost, relayTarget } from "./webApi";
import { parseProviderIcons, type ProviderIconDescriptor } from "./providerCatalogueModel";

let scope: string | undefined;
let icons: ProviderIconDescriptor[] = [];
const subscribers = new Set<() => void>();

export function subscribeProviderIcons(fn: () => void): () => void {
  subscribers.add(fn);
  window.addEventListener("codeterm-peer-switch-rebind", fn);
  return () => {
    subscribers.delete(fn);
    window.removeEventListener("codeterm-peer-switch-rebind", fn);
  };
}

function cacheKey(): string {
  return `codeterm-provider-icons:${apiHost()}:${relayTarget()?.id ?? "self"}`;
}

export function cachedProviderIcon(id: string | null | undefined): ProviderIconDescriptor | undefined {
  try {
    const key = cacheKey();
    if (scope !== key) {
      scope = key;
      icons = [];
      icons = parseProviderIcons(localStorage.getItem(key));
    }
  } catch { icons = []; scope = undefined; }
  return icons.find((icon) => icon.id === id);
}

export function persistProviderIcons(providers: ProviderIconDescriptor[]): void {
  try {
    scope = cacheKey();
    icons = providers.map(({ id, display_name, icon_svg, icon_hint, icon_color }) =>
      ({ id, display_name, icon_svg, icon_hint, icon_color }));
    localStorage.setItem(scope, JSON.stringify(icons));
  } catch { /* The live catalogue remains usable without storage. */ }
  for (const fn of subscribers) fn();
}
