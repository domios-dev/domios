// Mirrored by agent_claude/provider/folder_trust.rs; parity-tested on shared fixtures.

const FRAME_EDGES = /^[\s│┃║╭╮╰╯┌┐└┘╔╗╚╝─━═]+|[\s│┃║╭╮╰╯┌┐└┘╔╗╚╝─━═]+$/g;
const ENTER = "DQ==";
const MOVE_DOWN = "ag==";
const MOVE_UP = "aw==";

interface OptionRow {
  focused: boolean;
  label: string;
}

function frameContent(line: string): string {
  return line.replace(FRAME_EDGES, "");
}

function isPathRow(row: string): boolean {
  return /^(?:\/[^ ]|~[\/\\]|\\\\|[A-Za-z]:[\\\/])/.test(row);
}

function pointerLabel(row: string): string | null {
  const m = /^[❯>]\s+/.exec(row);
  return m ? row.slice(m[0].length) : null;
}

function stripIndex(label: string): string {
  const m = /^\d+\.\s+/.exec(label);
  return m ? label.slice(m[0].length) : label;
}

function optionRow(row: string): OptionRow {
  const pointed = pointerLabel(row);
  return { focused: pointed !== null, label: stripIndex(pointed !== null ? pointed : row) };
}

function leadsWith(label: string, word: string): boolean {
  return new RegExp("^" + word + "(?:$|[, .])", "i").test(label);
}

// `null` when no trust dialog is shown; `[]` holds a trust dialog whose shape
// does not prove which key advances it toward trusting the folder.
export function claudeFolderTrustResponse(screen: string): string[] | null {
  const rows = String(screen || "").split("\n").map(frameContent);
  const path = rows.findIndex(isPathRow);
  if (path < 0) return null;
  let trust = -1;
  for (let i = path + 1; i < rows.length; i++) {
    if (rows[i].toLowerCase().indexOf("trust") !== -1) { trust = i; break; }
  }
  if (trust < 0) return null;
  let pointer = false;
  for (let i = trust + 1; i < rows.length; i++) {
    if (pointerLabel(rows[i]) !== null) { pointer = true; break; }
  }
  if (!pointer) return null;
  return answer(rows, path, trust) || [];
}

function answer(rows: string[], path: number, trust: number): string[] | null {
  if (!rows.slice(0, path).some((row) => row.endsWith(":"))) return null;
  let footer = rows.length - 1;
  while (footer >= 0 && !rows[footer]) footer--;
  if (footer < 0) return null;
  const footerText = rows[footer].toLowerCase();
  if (footerText.indexOf("enter") === -1 || footerText.indexOf("esc") === -1) return null;
  let end = footer;
  while (end > 0 && !rows[end - 1]) end--;
  let start = end;
  while (start > 0 && rows[start - 1]) start--;
  if (end - start !== 2 || trust >= start) return null;
  const options = rows.slice(start, end).map(optionRow);
  if (options.filter((option) => option.focused).length !== 1) return null;
  const focused = options.findIndex((option) => option.focused);
  const yes = options.findIndex((option) => leadsWith(option.label, "yes"));
  const no = options.findIndex((option) => leadsWith(option.label, "no"));
  if (yes < 0 || no < 0 || options[no].label.toLowerCase().indexOf("exit") === -1) return null;
  if (focused === yes) return [ENTER];
  return [yes > focused ? MOVE_DOWN : MOVE_UP];
}
