import { useSyncExternalStore } from "react";

export interface PwaInstallOffer {
  kind: "prompt" | "ios";
  description: string;
  install?: () => Promise<void>;
  dismiss: () => void;
}

let offer: PwaInstallOffer | null = null;
const listeners = new Set<() => void>();

export function setPwaInstallOffer(next: PwaInstallOffer | null) {
  offer = next;
  for (const listener of listeners) listener();
}

export function getPwaInstallOffer(): PwaInstallOffer | null {
  return offer;
}

export function usePwaInstallOffer(): PwaInstallOffer | null {
  return useSyncExternalStore(
    (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
    () => offer,
    () => null,
  );
}
