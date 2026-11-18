export function readPickerGroupExpansion(providerId: string): ReadonlyMap<string, boolean> {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(`ct-model-picker-groups:${providerId}`) ?? "{}");
    if (parsed && typeof parsed === "object") {
      return new Map(Object.entries(parsed).filter((entry): entry is [string, boolean] => typeof entry[1] === "boolean"));
    }
  } catch {}
  return new Map();
}

export function writePickerGroupExpansion(providerId: string, groups: ReadonlyMap<string, boolean>): void {
  try {
    window.localStorage.setItem(`ct-model-picker-groups:${providerId}`, JSON.stringify(Object.fromEntries(groups)));
  } catch {}
}

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
