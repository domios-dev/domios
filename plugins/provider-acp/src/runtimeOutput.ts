import type { ChatMsg } from "@codeterm/plugin-sdk";
import { utf8Length } from "./runtimeReduceAccounting";

export const IMAGE_DATA_BUDGET = 256 * 1024;
export const TOOL_OUTPUT_BUDGET = 16 * 1024;
const REDUCTION_IMAGE_BUDGET = 3 * 1024 * 1024;

export function imageAdmission(): (bytes: number) => boolean {
  let remaining = REDUCTION_IMAGE_BUDGET;
  return bytes => {
    if (bytes > remaining) return false;
    remaining -= bytes;
    return true;
  };
}

export function inlineImage(mimeType: unknown, data: unknown): { mimeType: string; data: string } | undefined {
  if (typeof mimeType !== "string" || mimeType.length > 128 || !/^image\/[a-zA-Z0-9.+-]+$/.test(mimeType)
    || typeof data !== "string" || !data.length || !/^[A-Za-z0-9+/]+={0,2}$/.test(data)) return undefined;
  return { mimeType, data };
}

export function imageTooLarge(data: string): string {
  const bytes = Math.floor(data.length * 3 / 4) - (data.endsWith("==") ? 2 : data.endsWith("=") ? 1 : 0);
  return `[image too large (${Math.ceil(bytes / 1024)} KB)]`;
}

export function withoutImages(row: ChatMsg): ChatMsg {
  const { attachments: _attachments, ...textRow } = row;
  return textRow;
}

export function boundedOutput(value: string, budget: number): string {
  if (utf8Length(JSON.stringify(value)) <= budget) return value;
  const total = utf8Length(value);
  const sideBudget = Math.floor((budget - 64) / 2);
  let head = 0, tail = value.length, headBytes = 0, tailBytes = 0, cost = 0;
  for (const character of value) {
    cost += utf8Length(JSON.stringify(character)) - 2;
    if (cost > sideBudget) break;
    head += character.length;
    headBytes += utf8Length(character);
  }
  cost = 0;
  while (tail > head) {
    const last = value.charCodeAt(tail - 1);
    const start = last >= 0xdc00 && last <= 0xdfff && tail >= 2
      && value.charCodeAt(tail - 2) >= 0xd800 && value.charCodeAt(tail - 2) <= 0xdbff ? tail - 2 : tail - 1;
    const character = value.slice(start, tail);
    cost += utf8Length(JSON.stringify(character)) - 2;
    if (cost > sideBudget) break;
    tail = start;
    tailBytes += utf8Length(character);
  }
  return `${value.slice(0, head)}\n[${total - headBytes - tailBytes} bytes omitted]\n${value.slice(tail)}`;
}
