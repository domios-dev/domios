const STORAGE_KEY = "codeterm:session-picker-panes";

type Listener = () => void;
const listeners = new Set<Listener>();

function readIds(): Set<string> {
  try {
    const raw = window.sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((id): id is string => typeof id === "string" && id.length > 0));
  } catch {
    return new Set();
  }
}

function writeIds(ids: Set<string>): void {
  try {
    window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify([...ids]));
  } catch {
    // Session-only hint — ignore persistence failures.
  }
  for (const listener of listeners) listener();
}

export function subscribeSessionPickerPanes(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function markSessionPickerPane(paneId: string): void {
  const ids = readIds();
  ids.add(paneId);
  writeIds(ids);
}

export function clearSessionPickerPane(paneId: string): void {
  const ids = readIds();
  if (!ids.delete(paneId)) return;
  writeIds(ids);
}

export function isSessionPickerPane(paneId: string): boolean {
  return readIds().has(paneId);
}
