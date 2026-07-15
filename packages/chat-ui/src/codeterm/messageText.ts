export function normalizeMessageText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

export function decodeCliMessageText(text: string): string {
  return normalizeMessageText(text).replace(/\\r\\n|\\n|\\r/g, "\n");
}
