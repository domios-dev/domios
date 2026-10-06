export function normalizeMessageText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}
