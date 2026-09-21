/**
 * A map node points at an essence by anchor (`capability:workspaces-and-tabs`).
 * Vectors are headings; capabilities, core parts and clients are table rows. Without
 * this, every node opened the whole document at the top.
 */

function headingSection(lines: string[], target: string): string | null {
  for (let i = 0; i < lines.length; i += 1) {
    const heading = /^(#{1,6})\s+(.*)$/.exec(lines[i]);
    if (!heading || /\{#([^}]+)\}\s*$/.exec(heading[2])?.[1] !== target) continue;
    const level = heading[1].length;
    let end = lines.length;
    for (let j = i + 1; j < lines.length; j += 1) {
      const next = /^(#{1,6})\s+/.exec(lines[j]);
      if (next && next[1].length <= level) { end = j; break; }
    }
    const section = lines.slice(i, end);
    section[0] = section[0].replace(/\s+\{#[^}]+\}\s*$/, "");
    return section.join("\n").trim();
  }
  return null;
}

function tableRow(lines: string[], target: string): string | null {
  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].trimStart().startsWith("|")) continue;
    const first = splitRow(lines[i])[0] ?? "";
    if (first !== target) continue;
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

function splitRow(line: string): string[] {
  return line.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((cell) => cell.trim());
}

function isSeparator(line: string): boolean {
  if (!line.trimStart().startsWith("|")) return false;
  const cells = splitRow(line);
  return cells.length > 0 && cells.every((cell) => /^:?-+:?$/.test(cell));
}

/**
 * A seven-column table is unreadable in a drawer, so each row becomes its own
 * heading plus one bullet per column. Empty cells and "—" sinks are dropped.
 */
function tablesToBullets(markdown: string): string {
  const lines = markdown.split(/\r?\n/);
  const out: string[] = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].trimStart().startsWith("|") || !isSeparator(lines[i + 1] ?? "")) {
      out.push(lines[i]);
      continue;
    }
    const headers = splitRow(lines[i]);
    let row = i + 2;
    while (row < lines.length && lines[row].trimStart().startsWith("|") && !isSeparator(lines[row])) {
      const cells = splitRow(lines[row]);
      out.push(`**${headers[0] === "ID" ? cells[1] ?? cells[0] : cells[0]}**`, "");
      for (let column = headers[0] === "ID" ? 2 : 1; column < headers.length; column += 1) {
        const value = cells[column] ?? "";
        if (value && value !== "—") out.push(`- **${headers[column]}:** ${value}`);
      }
      out.push("");
      row += 1;
    }
    i = row - 1;
  }
  return out.join("\n").trim();
}

/** Returns the section an anchor names, or the whole document when nothing matches. */
export function extractEssenceSection(markdown: string, anchor: string): string {
  const target = anchor.trim();
  if (!/^(vector|capability|core|client):[a-z0-9]+(?:-[a-z0-9]+)*$/.test(target) || !markdown) return markdown;
  const lines = markdown.split(/\r?\n/);
  const section = headingSection(lines, target) ?? tableRow(lines, target) ?? markdown;
  return tablesToBullets(section);
}
