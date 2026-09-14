import { assignedCommand, toolCommand, type FileDiffHunk, type ToolEvent } from "@codeterm/chat-core";
import { useState } from "react";
import { ToolOutputContent } from "../ToolOutputContent";
import { BranchList, COMMAND_TREE_CAP, type Branch } from "./CommandTree";
import { DiffHunkLines } from "./FileDiffCards";

type BranchDetail =
  | { kind: "hunk"; hunk: FileDiffHunk; path: string }
  | { kind: "output"; text: string }
  | { kind: "assignment"; command: string };

interface MemberBranchModel {
  branches: Branch[];
  details: BranchDetail[];
}

const MAX_BRANCH_CHARS = 120;
const OUTPUT_WRAPPER = /^(?:exit code:?\s*-?\d+|wall time:?.*|output:|script completed|chunk id:.*|original token count:.*|total output lines:.*|process exited.*)$/i;
const MODELS = new WeakMap<ToolEvent, MemberBranchModel | null>();

function clip(text: string): string {
  return text.length > MAX_BRANCH_CHARS ? `${text.slice(0, MAX_BRANCH_CHARS - 1)}…` : text;
}

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

const STRUCTURE_ONLY = /^[\s{}[\]()<>,;:.'"`]*$/;
const KEY_VALUE = /^"([^"]+)"\s*:\s*(.*?),?$/;

function scalarText(value: unknown): string | undefined {
  if (typeof value === "string") return value.trim() || undefined;
  if (typeof value === "number" || typeof value === "boolean" || value === null) return String(value);
  return undefined;
}

/** The first scalar of a JSON document: `key value` for an object entry, the value itself in an array. */
function firstJsonScalar(value: unknown): string | undefined {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = scalarText(item) ?? firstJsonScalar(item);
      if (found) return found;
    }
    return undefined;
  }
  if (!value || typeof value !== "object") return undefined;
  for (const [key, item] of Object.entries(value)) {
    const scalar = scalarText(item);
    if (scalar !== undefined) return `${key} ${scalar}`;
  }
  for (const item of Object.values(value)) {
    const found = firstJsonScalar(item);
    if (found) return found;
  }
  return undefined;
}

function meaningfulLine(line: string): string | undefined {
  if (STRUCTURE_ONLY.test(line)) return undefined;
  const entry = KEY_VALUE.exec(line);
  if (!entry) return line;
  const value = entry[2].trim();
  if (!value || STRUCTURE_ONLY.test(value)) return undefined;
  return `${entry[1]} ${value.replace(/^"(.*)"$/, "$1")}`;
}

/** The first output line a reader cares about: wrapper and bare JSON structure skipped, JSON read as its first scalar. */
export function firstOutputLine(result: string | undefined): string | undefined {
  const lines = (result?.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").split(/\r?\n/) ?? []).map((line) => line.trim());
  const body = lines.filter((line) => line && !OUTPUT_WRAPPER.test(line));
  const document = body.join("\n");
  if (/^[{[]/.test(document)) {
    try {
      const scalar = firstJsonScalar(JSON.parse(document));
      return scalar ? clip(scalar.replace(/\s+/g, " ")) : undefined;
    } catch {
      // Not one JSON document; read it line by line.
    }
  }
  for (const line of body) {
    const meaningful = meaningfulLine(line);
    if (meaningful) return clip(meaningful);
  }
  return undefined;
}

function buildModel(member: ToolEvent): MemberBranchModel | null {
  const branches: Branch[] = [];
  const details: BranchDetail[] = [];
  const diffs = member.fileDiffs ?? [];
  for (const diff of diffs) {
    const path = diff.movePath ?? diff.path;
    for (const hunk of diff.hunks) {
      const changed = hunk.lines.find((line) => line.startsWith("+") || line.startsWith("-"));
      const lead = `@@ -${hunk.oldStart} +${hunk.newStart}`;
      branches.push({ kind: "hunk", lead: diffs.length > 1 ? `${baseName(path)} ${lead}` : lead, object: changed && clip(changed), dim: true });
      details.push({ kind: "hunk", hunk, path });
    }
  }
  if (diffs.length === 0 && member.kind === "command") {
    const { command, argv } = toolCommand(member);
    const assigned = assignedCommand(command, argv);
    if (assigned) {
      branches.push({ kind: "assignment", lead: `→ ${assigned.variable}` });
      details.push({ kind: "assignment", command });
    }
    const line = firstOutputLine(member.result);
    if (line && member.result) {
      branches.push({ kind: "output", lead: line, ...(member.exitCode ? { marker: `exit ${member.exitCode}` } : {}) });
      details.push({ kind: "output", text: member.result });
    }
  }
  return branches.length > 0 ? { branches, details } : null;
}

function memberBranchModel(member: ToolEvent): MemberBranchModel | null {
  if (!MODELS.has(member)) MODELS.set(member, buildModel(member));
  return MODELS.get(member) ?? null;
}

function BranchDetailView({ detail }: { detail: BranchDetail }) {
  if (detail.kind === "hunk") {
    return <div className="overflow-x-auto"><DiffHunkLines hunk={detail.hunk} path={detail.path} /></div>;
  }
  if (detail.kind === "output") {
    return <div data-testid="chat-branch-output"><ToolOutputContent text={detail.text} maxHeight="12rem" /></div>;
  }
  return (
    <pre data-testid="chat-branch-command" className="m-0 max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
      {detail.command}
    </pre>
  );
}

/** A collapsed stream member's inline branches: its hunks, first output line, or assigned variable; nothing when it has none. */
export function MemberBranches({ member, label }: { member: ToolEvent; label: string }) {
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const model = memberBranchModel(member);
  if (!model) return null;
  return (
    <BranchList
      branches={model.branches}
      cap={COMMAND_TREE_CAP}
      openIndex={openIndex}
      onBranch={(index) => setOpenIndex(openIndex === index ? null : index)}
      renderDetail={(index) => <BranchDetailView detail={model.details[index]} />}
      label={label}
    />
  );
}
