import type { PromptOption } from "./types";

export function encodePromptSelection(options: PromptOption[], selected: ReadonlySet<string>): string {
  if (options.length > 31 || options.some((option) => (!/^\d+$/.test(option.value) || Number(option.value) > 31))) {
    return JSON.stringify(options.filter((option) => selected.has(option.value)).map((option) => option.value));
  }
  let mask = 0;
  for (const option of options) {
    const index = Number(option.value) - 1;
    if (index >= 0 && index < 31 && selected.has(option.value)) mask += 2 ** index;
  }
  return String(10000 + mask);
}
