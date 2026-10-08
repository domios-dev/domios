export interface LineRange { start: number; end: number }

function plain(text: string): string {
  return text.replace(/[*_`#>[\]()|~]/g, "").replace(/\s+/g, " ").trim().toLowerCase();
}

/** Best-effort source lines for text selected in a rendered view, whose markup differs from the source. */
export function locateSelection(source: string, selection: string): LineRange | null {
  const lines = source.split(/\r?\n/).map(plain);
  const selected = selection.split(/\r?\n/).map(plain).filter(Boolean);
  if (!selected.length) return null;
  const find = (needle: string, from: number) => {
    const probe = needle.slice(0, 60);
    for (let index = from; index < lines.length; index += 1) if (lines[index].includes(probe)) return index;
    return -1;
  };
  const start = find(selected[0], 0);
  if (start < 0) return null;
  const end = selected.length > 1 ? find(selected[selected.length - 1], start) : start;
  return { start: start + 1, end: (end < 0 ? start : end) + 1 };
}

export function lineSuffix(range: LineRange | null): string {
  if (!range) return "";
  return range.end > range.start ? `:${range.start}-${range.end}` : `:${range.start}`;
}

export function quotedSelection(path: string, selection: string, range: LineRange | null): string {
  const quote = selection.replace(/\s+$/, "").split(/\r?\n/).map((line) => line.trim() ? `> ${line}` : ">").join("\n");
  return `From \`${path}${lineSuffix(range)}\`:\n${quote}`;
}
