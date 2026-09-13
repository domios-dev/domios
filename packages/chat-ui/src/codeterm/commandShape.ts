import {
  commandArgvStart,
  commandBasename,
  parseShellComposition,
  toolCommand,
  type ShellOperator,
  type ShellScript,
  type ShellSegment,
  type ToolEvent,
} from "@codeterm/chat-core";
import { invocationFromArgv, type CodetermInvocation } from "./invocation";

export type CommandRow =
  | { role: "codeterm"; segment: ShellSegment; invocation: CodetermInvocation }
  | { role: "script"; segment: ShellSegment; script: ShellScript }
  | { role: "command"; segment: ShellSegment };

/** How one command call is presented: a single row, a specialized card, or one grouped box of segments. */
export type CommandShape =
  | { kind: "single" }
  | { kind: "read" }
  | { kind: "search" }
  | { kind: "codeterm"; invocation: CodetermInvocation }
  | { kind: "group"; rows: CommandRow[] };

const SHAPE_CACHE_LIMIT = 500;
const SHAPES = new Map<string, CommandShape>();
const SINGLE: CommandShape = { kind: "single" };
const SEARCHERS = new Set(["rg", "grep"]);
const READERS = new Set(["cat", "head", "tail", "nl"]);
const OPERATOR_SYMBOLS: Record<ShellOperator, string> = { pipe: "|", and: "&&", or: "||", then: ";" };

export function operatorSymbol(operator: ShellOperator): string {
  return OPERATOR_SYMBOLS[operator];
}

function isShellOption(segment: ShellSegment): boolean {
  const [head, ...options] = segment.argv;
  return head === "set" && options.length > 0 && options.every((option) => /^[-+][A-Za-z]+$/.test(option) || /^[a-z]+$/.test(option));
}

function isSetup(segment: ShellSegment): boolean {
  if (segment.redirects.length > 0 || segment.script) return false;
  const { argv } = segment;
  const head = argv[0];
  if (/^(?:cd|set-location|pushd|chdir)$/i.test(head)) return argv.length === 2;
  if (isShellOption(segment)) return true;
  const assignments = head === "export" ? argv.slice(1) : argv;
  if (assignments.length > 0 && assignments.every((word) => /^[A-Za-z_][A-Za-z0-9_]*=/.test(word))) return true;
  if (!/^\$env:[A-Za-z_][A-Za-z0-9_]*(?:=|$)/i.test(head)) return false;
  return argv.length === 1 ? head.includes("=") : argv.length <= 3 && argv[1].startsWith("=");
}

function readerStage(segment: ShellSegment): "read" | "search" | null {
  if (segment.script) return null;
  let start = commandArgvStart(segment.argv);
  if (segment.argv[start] === "command") start++;
  const name = segment.argv[start] == null ? "" : commandBasename(segment.argv[start]);
  if (SEARCHERS.has(name)) return "search";
  if (READERS.has(name) || (name === "sed" && segment.argv[start + 1] === "-n")) return "read";
  return null;
}

// A pipeline's effect is its whole chain: `cat x | codeterm mem save` writes, so every stage must read.
function readerKind(segments: ShellSegment[]): "read" | "search" | null {
  const chains: ShellSegment[][] = [];
  let chain: ShellSegment[] = [];
  for (const segment of segments) {
    chain.push(segment);
    if (segment.operator !== "pipe") {
      chains.push(chain);
      chain = [];
    }
  }
  const relevant = chains.filter((stages) => !(stages.length === 1 && isShellOption(stages[0])));
  if (relevant.length === 0 || !relevant.every((stages) => stages.every(readerStage))) return null;
  const heads = relevant.map((stages) => readerStage(stages[0]));
  if (heads.every((kind) => kind === "search")) return "search";
  if (heads.every((kind) => kind === "read")) return "read";
  return null;
}

function rowOf(segment: ShellSegment): CommandRow {
  if (segment.script) return { role: "script", segment, script: segment.script };
  const invocation = invocationFromArgv(segment.argv, segment.text);
  return invocation ? { role: "codeterm", segment, invocation } : { role: "command", segment };
}

function computeShape(command: string, argv?: readonly string[]): CommandShape {
  const composition = parseShellComposition(command, argv);
  if (!composition) return SINGLE;
  const { segments } = composition;
  const reader = readerKind(segments);
  if (reader) return { kind: reader };
  const actions = segments.filter((segment) => !isSetup(segment));
  if (actions.length === 0) return SINGLE;
  if (actions.length === 1 && !actions[0].script) {
    const index = segments.indexOf(actions[0]);
    if (actions[0].operator !== "pipe" && segments[index - 1]?.operator !== "pipe") {
      const invocation = invocationFromArgv(actions[0].argv, actions[0].text);
      return invocation ? { kind: "codeterm", invocation } : SINGLE;
    }
  }
  return { kind: "group", rows: segments.map(rowOf) };
}

/** Memoized per exact command text and argv; a parser failure always yields the single-row shape. */
export function commandShape(command: string, argv?: readonly string[]): CommandShape {
  if (!command) return SINGLE;
  const key = argv ? JSON.stringify([argv, command]) : command;
  const cached = SHAPES.get(key);
  if (cached) return cached;
  let shape: CommandShape;
  try {
    shape = computeShape(command, argv);
  } catch {
    shape = SINGLE;
  }
  if (SHAPES.size >= SHAPE_CACHE_LIMIT) SHAPES.delete(SHAPES.keys().next().value as string);
  SHAPES.set(key, shape);
  return shape;
}

export function toolCommandShape(event: ToolEvent): CommandShape {
  const { command, argv } = toolCommand(event);
  return commandShape(command, argv);
}

export function commandRowLabel(row: CommandRow): string {
  if (row.role === "codeterm") return `codeterm ${row.invocation.path.join(" ")}`;
  const { argv } = row.segment;
  const start = commandArgvStart(argv);
  const name = argv[start] == null ? row.segment.text : commandBasename(argv[start]);
  if (row.role === "script") return `${name} (${row.script.lineCount} ${row.script.lineCount === 1 ? "line" : "lines"})`;
  const argument = argv[start + 1];
  return argument ? `${name} ${argument.length > 24 ? `${argument.slice(0, 23)}…` : argument}` : name;
}

/** A short header for a grouped call: its first meaningful segment plus how many more follow, never the raw chain. */
export function commandGroupSummary(rows: CommandRow[]): string {
  const lead = rows.find((row) => row.role !== "command") ?? rows[0];
  return rows.length > 1 ? `${commandRowLabel(lead)} +${rows.length - 1}` : commandRowLabel(lead);
}
