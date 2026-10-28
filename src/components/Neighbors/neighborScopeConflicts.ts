import type { NeighborPermission } from "./neighborRuntimeClient";

/** A place where the written scope promises something the switches below it do not grant. */
export interface ScopeConflict { grant: "memory" | "files" | "tabs" | "writes" | "folder"; line: string }

// The concierge follows the switches, not the prose, so a scope that claims more only misleads the owner.
// Each check looks for a mention that is not negated in its own clause ("may not read my memory" is fine).
const MEMORY = /\b(?:memory|memories)\b|памят/i;
const FILES = /\b(?:files?|documents?|attachments?)\b|файл|документ/i;
const TABS = /\b(?:tabs?|terminals?|run (?:a |the )?commands?)\b|вкладк|терминал/i;
// Changing verbs claim a write on any work object; writing and creating also describe prose ("write a summary of the
// project"), so those count only with an object that is itself files or code.
const CHANGE_VERB = /\b(?:edit|change|modify|commit|delete|remove|fix|refactor|push)\b|измен|редакт|исправ|удал/i;
const WORK_OBJECT = /\b(?:files?|code|repo(?:sitory)?|project|folder|directory|branch)\b|файл|код|репозитор|проект|папк/i;
const AUTHOR_VERB = /\b(?:write|create)\b|созда|запис/i;
const CODE_OBJECT = /\b(?:files?|code|branch)\b|файл|код/i;
const NEGATION = /\b(?:not|never|no|nothing|without|except|don't|doesn't|cannot|can't|mustn't)\b|(?:^|\s)(?:не|нельзя|без|никогда)(?=\s|$)/i;
// Absolute paths as people paste them: C:\work\x, C:/work/x, /home/x, ~/x. A trailing sentence mark is not part of the path;
// "and/or" and the "//host/x" of a URL are not paths.
const PATH = /(?:(?<![\w])[a-z]:[\\/]|(?<![\w.~:/])~[\\/]|(?<![\w.~:/])\/(?=[\w.-]))[^\s"'`<>|,;]*[^\s"'`<>|,;.:!?)]/gi;

function clauses(scope: string): string[] {
  return scope.split(/[.;!?\n]+|,\s*(?:but|except|however)\b/i).map(part => part.trim()).filter(Boolean);
}
/** The clause mentions the thing, and no negation comes before the mention in that clause. */
function grants(clause: string, mention: RegExp): boolean {
  const found = mention.exec(clause);
  return found !== null && !NEGATION.test(clause.slice(0, found.index));
}
const mentions = (scope: string, mention: RegExp) => clauses(scope).some(clause => grants(clause, mention));

/** A path's root and its segments with "." and ".." resolved lexically; null when ".." climbs above the root. */
function lexical(path: string): { root: string; segments: string[]; windows: boolean } | null {
  const slashed = path.replace(/\\/g, "/");
  const drive = /^([a-z]):\//i.exec(slashed);
  const root = drive ? `${drive[1].toLowerCase()}:` : slashed.startsWith("~/") ? "~" : "/";
  const segments: string[] = [];
  for (const part of slashed.slice(drive ? 3 : root === "~" ? 2 : 1).split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") { if (!segments.pop()) return null; continue; }
    segments.push(part);
  }
  return { root, segments, windows: drive !== null };
}
/** Inside the folder, or the folder itself, compared lexically: Windows drive paths without case, as that file system
 *  does, everything else with case. Null when the comparison cannot be made honestly (a "~" path against a folder
 *  named another way, since home is not known here). */
function within(path: string, folder: string): boolean | null {
  const target = lexical(path), root = lexical(folder);
  if (!root) return null;
  if (!target) return false;
  if ((target.root === "~") !== (root.root === "~")) return null;
  if (target.root !== root.root || target.segments.length < root.segments.length) return false;
  const same = (a: string, b: string) => root.windows ? a.toLowerCase() === b.toLowerCase() : a === b;
  return root.segments.every((segment, index) => same(segment, target.segments[index]));
}

/** What the scope text claims beyond the structured grants, as notices to read before Apply. Never blocks Apply. */
export function scopeConflicts(permission: NeighborPermission): ScopeConflict[] {
  const scope = permission.scope;
  if (!permission.allowed || !scope.trim()) return [];
  const found: ScopeConflict[] = [];
  if (permission.allow_memory === false && mentions(scope, MEMORY))
    found.push({ grant: "memory", line: "The scope mentions your memory, but Read my memory is off." });
  if (permission.allow_files === false && mentions(scope, FILES))
    found.push({ grant: "files", line: "The scope mentions files, but Read files is off." });
  if (!permission.allow_tab_requests && mentions(scope, TABS))
    found.push({ grant: "tabs", line: "The scope mentions tabs or commands, but Open tabs and agents is off." });
  if ((permission.machine_access ?? "read_only") === "read_only"
    && clauses(scope).some(clause => (grants(clause, CHANGE_VERB) && WORK_OBJECT.test(clause)) || (grants(clause, AUTHOR_VERB) && CODE_OBJECT.test(clause))))
    found.push({ grant: "writes", line: "The scope asks for changes, but machine access is Read only." });
  if (permission.machine_access === "folder" && permission.folder) {
    const folder = permission.folder;
    // Only a path known to be outside is named; one that cannot be compared is left alone rather than guessed.
    const outside = [...new Set(scope.match(PATH) ?? [])].filter(path => within(path, folder) === false);
    if (outside.length) found.push({ grant: "folder", line: `The scope names ${outside.length === 1 ? outside[0] : `${outside.length} paths`} outside the allowed folder.` });
  }
  return found;
}
