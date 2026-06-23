import type { CompletionItem, ParsedInput } from "./types";

export function getCompletionInsert(parsed: ParsedInput, item: CompletionItem): string | null {
  if (!isSafeCompletionText(item.text)) return null;

  const prefix = parsed.currentArg;
  if (item.text.toLowerCase().startsWith(prefix.toLowerCase())) {
    return item.text.slice(prefix.length) + " ";
  }

  return null;
}

export function isSafeCompletionText(text: string): boolean {
  return text.length > 0 && !/[\x00-\x1f\x7f\x1b]/.test(text);
}
