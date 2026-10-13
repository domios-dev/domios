export function thoughtPresentation(text: string): { headline: string; expandable: boolean } {
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const headline = (lines[0] ?? "").replace(/^\*\*(.+)\*\*$/, "$1").trim();
  return { headline, expandable: Boolean(headline) && lines.length > 1 };
}
