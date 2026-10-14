import type { GrantRequest } from "./pluginConsentModel";

interface PendingConsent { request: GrantRequest; resolve: (allowed: boolean) => void }
const queue: PendingConsent[] = [];
const listeners = new Set<() => void>();
const emit = () => { for (const listener of listeners) listener(); };
export const getPluginConsent = () => queue[0]?.request ?? null;
export function subscribePluginConsent(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export function requestPluginConsent(request: GrantRequest): Promise<boolean> {
  return new Promise((resolve) => { queue.push({ request, resolve }); emit(); });
}
export function decidePluginConsent(allowed: boolean, expected = getPluginConsent()) {
  if (queue[0]?.request !== expected) return;
  queue.shift()?.resolve(allowed);
  emit();
}
