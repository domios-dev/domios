import type { NeighborPermission } from "./neighborRuntimeClient";

/** A place where the written scope promises something the switches below it do not grant. */
export interface ScopeConflict { grant: "memory" | "files" | "tabs" | "writes" | "folder"; line: string }

// The concierge follows the switches, not the prose, so a scope that claims more only misleads the owner.
// Each check looks for a mention that is not negated in its own clause ("may not read my memory" is fine).
const MEMORY = /\b(?:memory|memories)\b|памят/i;
const FILES = /\b(?:files?|documents?|attachments?)\b|файл|документ/i;
const TABS = /\b(?:tabs?|terminals?|run (?:a |the )?commands?)\b|вкладк|терминал/i;
const WRITE_VERB = /\b(?:edit|write|change|modify|commit|delete|remove|create|fix|refactor|push)\b|измен|редакт|исправ|удал|созда|запис/i;
const WRITE_OBJECT = /\b(?:files?|code|repo(?:sitory)?|project|folder|directory|branch)\b|файл|код|репозитор|проект|папк/i;
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

function normalized(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}
/** Inside the folder, or the folder itself; Windows paths compare without case, as the file system does. */
function within(path: string, folder: string): boolean {
  const target = normalized(path), root = normalized(folder);
  return target === root || target.startsWith(`${root}/`);
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
    && clauses(scope).some(clause => grants(clause, WRITE_VERB) && WRITE_OBJECT.test(clause)))
    found.push({ grant: "writes", line: "The scope asks for changes, but machine access is Read only." });
  if (permission.machine_access === "folder" && permission.folder) {
    const folder = permission.folder;
    const outside = [...new Set(scope.match(PATH) ?? [])].filter(path => !within(path, folder));
    if (outside.length) found.push({ grant: "folder", line: `The scope names ${outside.length === 1 ? outside[0] : `${outside.length} paths`} outside the allowed folder.` });
  }
  return found;
}
