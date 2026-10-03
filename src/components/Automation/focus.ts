export function focusLost(): boolean {
  const active = document.activeElement;
  return !active || active === document.body || !active.isConnected;
}

export function focusEntry(container: HTMLElement | null, key: string, value: string) {
  Array.from(container?.querySelectorAll<HTMLElement>("button") ?? []).find((entry) => entry.dataset[key] === value)?.focus();
}
