// Mirrored by agent_claude/provider/folder_trust.rs; parity-tested on shared fixtures.

const FRAME_EDGES = /^[\s│┃║╭╮╰╯┌┐└┘╔╗╚╝─━═]+|[\s│┃║╭╮╰╯┌┐└┘╔╗╚╝─━═]+$/g;
const ENTER = "DQ==";
const MOVE_DOWN = "ag==";
const MOVE_UP = "aw==";

interface OptionRow {
  focused: boolean;
  label: string;
}

interface TrustHeader {
  start: number;
  end: number;
  question: string;
}

interface TrustChoices {
  options: OptionRow[];
  footer: number;
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
  const header = findTrustHeader(rows);
  if (header && rows.slice(header.end + 1).some((row) => pointerLabel(row) !== null)) {
    if (hasUntitledLeadingPath(rows, header.start)) return [];
    const choices = choiceRows(rows, header.end);
    if (choices && !hasFolderTrustAffirmative(choices.options)) return null;
    const hasPath = choices
      ? rows.slice(header.start, choices.footer + 1).some(isPathRow)
      : rows.slice(header.start).some(isPathRow);
    return (choices ? answer(choices.options, hasPath) : null) || [];
  }

  const path = rows.findIndex(isPathRow);
  if (path >= 0) {
    let trust = -1;
    for (let i = path + 1; i < rows.length; i++) {
      if (rows[i].toLowerCase().indexOf("trust") !== -1) { trust = i; break; }
    }
    if (trust < 0 || !rows.slice(trust + 1).some((row) => pointerLabel(row) !== null)) return null;
    if (rows.slice(0, path).some((row) => row.endsWith(":"))) {
      const choices = choiceRows(rows, trust);
      if (choices) return answer(choices.options, false) || [];
    }
    return [];
  }
  return null;
}

function answer(options: OptionRow[], hasPath: boolean): string[] | null {
  if (hasPath || options.length !== 2) return null;
  if (!trustChoices(options)) return null;
  if (options.filter((option) => option.focused).length !== 1) return null;
  const focused = options.findIndex((option) => option.focused);
  const yes = options.findIndex((option) => leadsWith(option.label, "yes"));
  if (focused === yes) return [ENTER];
  return [yes > focused ? MOVE_DOWN : MOVE_UP];
}

export function claudeFolderTrustPrompt(screen: string): { question: string; options: [string, string][] } | null {
  const rows = String(screen || "").split("\n").map(frameContent);
  const prompt = pathlessPrompt(rows);
  if (!prompt || prompt.hasPath) return null;
  const options = prompt.choices.options;
  if (options.length !== 2) return null;
  if (options.filter((option) => option.focused).length !== 1 || !trustChoices(options)) return null;
  return {
    question: prompt.header.question,
    options: options.map((option, index): [string, string] => [option.label, String(index + 1)]),
  };
}

export function claudeFolderTrustHighlightedOptionIndex(screen: string): number | null {
  const rows = String(screen || "").split("\n").map(frameContent);
  const prompt = pathlessPrompt(rows);
  if (!prompt || prompt.hasPath || prompt.choices.options.length !== 2) return null;
  if (prompt.choices.options.filter((option) => option.focused).length !== 1 || !trustChoices(prompt.choices.options)) return null;
  return prompt.choices.options.findIndex((option) => option.focused);
}

function pathlessPrompt(rows: string[]): { header: TrustHeader; choices: TrustChoices; hasPath: boolean } | null {
  const header = findTrustHeader(rows);
  if (!header || hasUntitledLeadingPath(rows, header.start)) return null;
  const choices = choiceRows(rows, header.end);
  if (!choices) return null;
  return {
    header,
    choices,
    hasPath: rows.slice(header.start, choices.footer + 1).some(isPathRow),
  };
}

function hasUntitledLeadingPath(rows: string[], headerStart: number): boolean {
  const leadingRows = rows.slice(0, headerStart);
  const path = leadingRows.findIndex(isPathRow);
  return path >= 0 && !leadingRows.slice(0, path).some((row) => row.endsWith(":"));
}

function findTrustHeader(rows: string[]): TrustHeader | null {
  for (let start = 0; start < rows.length; start++) {
    const header = trustHeader(rows, start);
    if (header) return header;
  }
  return null;
}

function trustHeader(rows: string[], start: number): TrustHeader | null {
  const colon = rows[start].indexOf(":");
  if (colon < 1) return null;
  const title = rows[start].slice(0, colon).trim();
  if (!title) return null;
  let question = rows[start].slice(colon + 1).trim();
  let end = start;
  while (!isTrustQuestion(question)) {
    const next = rows[end + 1];
    if (!next || isFooter(next) || isPathRow(next) || pointerLabel(next) !== null || isTrustOption(optionRow(next).label)) {
      return null;
    }
    question += ` ${next.trim()}`;
    end++;
  }
  return { start, end, question: `${title}: ${question}`.replace(/\s+/g, " ").trim() };
}

function isTrustQuestion(question: string): boolean {
  return question.toLowerCase().indexOf("trust") !== -1 && question.indexOf("?") !== -1;
}

function isFooter(row: string): boolean {
  const footer = row.toLowerCase();
  return footer.indexOf("enter") !== -1 && footer.indexOf("esc") !== -1;
}

function choiceRows(rows: string[], trustEnd: number): TrustChoices | null {
  const footer = rows.findIndex((row, index) => index > trustEnd && isFooter(row));
  if (footer < 0) return null;
  const start = rows.findIndex(
    (row, index) => index > trustEnd && index < footer && isTrustOption(optionRow(row).label),
  );
  if (start < 0) return null;
  const options: OptionRow[] = [];
  for (let i = start; i < footer; i++) {
    if (!rows[i]) continue;
    const option = optionRow(rows[i]);
    if (!isTrustOption(option.label)) return null;
    options.push(option);
  }
  return { options, footer };
}

function trustChoices(options: OptionRow[]): boolean {
  const no = options.findIndex((option) => leadsWith(option.label, "no"));
  return hasFolderTrustAffirmative(options)
    && no >= 0
    && options[no].label.toLowerCase().indexOf("exit") !== -1;
}

function hasFolderTrustAffirmative(options: OptionRow[]): boolean {
  const yes = options.findIndex((option) => leadsWith(option.label, "yes"));
  return yes >= 0 && /\btrust\b/i.test(options[yes].label);
}

function isTrustOption(label: string): boolean {
  return leadsWith(label, "yes") || leadsWith(label, "no");
}
