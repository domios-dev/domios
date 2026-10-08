const prefix = "codeterm:tab-panel:";

export function readTabPanel<T>(kind: string, tabId: string, validate: (value: unknown) => value is T): T | null {
  try {
    const raw = window.localStorage.getItem(`${prefix}${kind}:${tabId}`);
    const value: unknown = raw ? JSON.parse(raw) : null;
    return validate(value) ? value : null;
  } catch { return null; }
}

export function writeTabPanel(kind: string, tabId: string, value: unknown) {
  try {
    const key = `${prefix}${kind}:${tabId}`;
    if (value === null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, JSON.stringify(value));
  } catch { /* A blocked browser store must not prevent panel navigation. */ }
}
