/**
 * A map node points at an essence by anchor (`capability:workspaces-and-tabs`).
 * Vectors are headings; capabilities, core parts and clients are table rows. Without
 * this, every node opened the whole document at the top.
 */

function slug(value: string): string {
  return value
    .toLowerCase()
    .replace(/^\d+[.)]\s*/, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function headingSection(lines: string[], target: string): string | null {
  for (let i = 0; i < lines.length; i += 1) {
    const heading = /^(#{1,6})\s+(.*)$/.exec(lines[i]);
    if (!heading || slug(heading[2]) !== target) continue;
    const level = heading[1].length;
    let end = lines.length;
    for (let j = i + 1; j < lines.length; j += 1) {
      const next = /^(#{1,6})\s+/.exec(lines[j]);
      if (next && next[1].length <= level) { end = j; break; }
    }
    return lines.slice(i, end).join("\n").trim();
  }
  return null;
}

function tableRow(lines: string[], target: string): string | null {
  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].trimStart().startsWith("|")) continue;
    const first = lines[i].split("|")[1]?.trim() ?? "";
    if (slug(first) !== target) continue;
    let header = i;
    for (let j = i - 1; j >= 0; j -= 1) {
      if (!lines[j].trimStart().startsWith("|")) break;
      header = j;
    }
    // Keep the header and its separator so the row's columns still mean something.
    if (header + 1 < i) return [lines[header], lines[header + 1], lines[i]].join("\n");
    return lines[i];
  }
  return null;
}

/** Returns the section an anchor names, or the whole document when nothing matches. */
export function extractEssenceSection(markdown: string, anchor: string): string {
  const target = slug(anchor.replace(/^[a-z]+:/i, ""));
  if (!target || !markdown) return markdown;
  const lines = markdown.split(/\r?\n/);
  return headingSection(lines, target) ?? tableRow(lines, target) ?? markdown;
}
