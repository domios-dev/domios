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

function sameIcons(left: ProviderIconDescriptor[], right: ProviderIconDescriptor[]): boolean {
  return left.length === right.length && left.every((icon, index) => {
    const next = right[index];
    return icon.id === next.id && icon.display_name === next.display_name && icon.icon_svg === next.icon_svg &&
      icon.icon_hint === next.icon_hint && icon.icon_color === next.icon_color;
  });
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
  const next = providers.map(({ id, display_name, icon_svg, icon_hint, icon_color }) =>
    ({ id, display_name, icon_svg, icon_hint, icon_color }));
  let nextScope: string | undefined;
  try {
    nextScope = cacheKey();
  } catch {
    nextScope = undefined;
  }
  const changed = !sameIcons(icons, next);
  const scopeChanged = scope !== nextScope;
  if (!changed && !scopeChanged) return;
  scope = nextScope;
  icons = next;
  if (scope) {
    try { localStorage.setItem(scope, JSON.stringify(icons)); } catch {}
  }
  if (changed) for (const fn of subscribers) fn();
}
