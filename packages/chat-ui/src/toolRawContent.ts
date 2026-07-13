export function distinctToolRawContent(
  content: string | null | undefined,
  primary: string | null | undefined,
  inputText: string,
): string | null {
  const raw = (content ?? "").trim();
  if (!raw) return null;
  if (raw === primary) return null;
  if (raw === inputText) return null;
  return raw;
}
