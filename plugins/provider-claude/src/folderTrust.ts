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
  if (path >= 0) {
    let trust = -1;
    for (let i = path + 1; i < rows.length; i++) {
      if (rows[i].toLowerCase().indexOf("trust") !== -1) { trust = i; break; }
    }
    if (trust < 0 || !rows.slice(trust + 1).some((row) => pointerLabel(row) !== null)) return null;
    return answer(rows, path, trust) || [];
  }

  const trust = rows.findIndex(isPathlessTrustHeader);
  if (trust < 0 || !rows.slice(trust + 1).some((row) => pointerLabel(row) !== null)) return null;
  return answer(rows, null, trust) || [];
}

function answer(rows: string[], path: number | null, trust: number): string[] | null {
  if (path !== null && !rows.slice(0, path).some((row) => row.endsWith(":"))) return null;
  if (path === null && !isPathlessTrustHeader(rows[trust])) return null;
  const options = choiceRows(rows, trust);
  if (!options || options.length !== 2) return null;
  if (options.filter((option) => option.focused).length !== 1) return null;
  const focused = options.findIndex((option) => option.focused);
  const yes = options.findIndex((option) => leadsWith(option.label, "yes"));
  const no = options.findIndex((option) => leadsWith(option.label, "no"));
  if (yes < 0 || no < 0 || options[no].label.toLowerCase().indexOf("exit") === -1) return null;
  if (focused === yes) return [ENTER];
  return [yes > focused ? MOVE_DOWN : MOVE_UP];
}

export function claudeFolderTrustPrompt(screen: string): { question: string; options: [string, string][] } | null {
  const rows = String(screen || "").split("\n").map(frameContent);
  if (rows.some(isPathRow)) return null;
  const trust = rows.findIndex(isPathlessTrustHeader);
  if (trust < 0) return null;
  const options = choiceRows(rows, trust);
  if (!options || options.length !== 2) return null;
  if (options.filter((option) => option.focused).length !== 1 || !trustChoices(options)) return null;
  return {
    question: rows[trust].trim(),
    options: options.map((option, index): [string, string] => [option.label, String(index + 1)]),
  };
}

export function claudeFolderTrustHighlightedOptionIndex(screen: string): number | null {
  const parsed = claudeFolderTrustPrompt(screen);
  if (!parsed) return null;
  const rows = String(screen || "").split("\n").map(frameContent);
  const trust = rows.findIndex(isPathlessTrustHeader);
  return choiceRows(rows, trust)?.findIndex((option) => option.focused) ?? null;
}

function choiceRows(rows: string[], trust: number): OptionRow[] | null {
  let footer = rows.length - 1;
  while (footer >= 0 && !rows[footer]) footer--;
  if (footer < 0) return null;
  const footerText = rows[footer].toLowerCase();
  if (footerText.indexOf("enter") === -1 || footerText.indexOf("esc") === -1) return null;
  const start = rows.findIndex(
    (row, index) => index > trust && index < footer && isTrustOption(optionRow(row).label),
  );
  if (start < 0) return null;
  const options: OptionRow[] = [];
  for (let i = start; i < footer; i++) {
    if (!rows[i]) continue;
    const option = optionRow(rows[i]);
    if (!isTrustOption(option.label)) return null;
    options.push(option);
  }
  return options;
}

function isPathlessTrustHeader(row: string): boolean {
  const colon = row.indexOf(":");
  if (colon < 1) return false;
  const header = row.slice(0, colon).trim();
  const question = row.slice(colon + 1).toLowerCase();
  return header.length > 0
    && question.indexOf("trust") !== -1
    && question.indexOf("?") !== -1;
}

function trustChoices(options: OptionRow[]): boolean {
  const yes = options.findIndex((option) => leadsWith(option.label, "yes"));
  const no = options.findIndex((option) => leadsWith(option.label, "no"));
  return yes >= 0 && no >= 0 && options[no].label.toLowerCase().indexOf("exit") !== -1;
}

function isTrustOption(label: string): boolean {
  return leadsWith(label, "yes") || leadsWith(label, "no");
}
