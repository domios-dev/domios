export function normalizeMessageText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

const WINDOWS_PATH_TOKEN = /(?:[A-Za-z]:\\[^\s"'`]+|\\\\[^\s"'`]+)/g;

export function decodeCliMessageText(text: string): string {
  const normalized = normalizeMessageText(text);
  let decoded = "";
  let offset = 0;

  for (const match of normalized.matchAll(WINDOWS_PATH_TOKEN)) {
    const index = match.index ?? 0;
    decoded += decodeProseEscapes(normalized.slice(offset, index));
    decoded += match[0];
    offset = index + match[0].length;
  }

  return decoded + decodeProseEscapes(normalized.slice(offset));
}

function decodeProseEscapes(text: string): string {
  return text.replace(/\\r\\n|\\n/g, "\n");
}
