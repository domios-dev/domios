export function readPickerShowAll(providerId: string): boolean {
  try {
    return window.localStorage.getItem(`ct-model-picker-show-all:${providerId}`) === "true";
  } catch {
    return false;
  }
}

export function writePickerShowAll(providerId: string, showAll: boolean): void {
  try {
    window.localStorage.setItem(`ct-model-picker-show-all:${providerId}`, String(showAll));
  } catch {}
}
