import { kindDisplayName, roleDisplayLabel } from "../roleKind";
import type { TemplateSummary } from "./startModel";

type TemplateAsk = import("../../../gen/TemplateAsk").TemplateAsk;
type TemplateUpdateDiff = import("../../../gen/TemplateUpdateDiff").TemplateUpdateDiff;
type TemplatePayloadItem = import("../../../gen/TemplatePayloadItem").TemplatePayloadItem;
type PackageAuthor = import("../../../gen/PackageAuthor").PackageAuthor;

/** "by Jane Doe, Example Labs", or null when the package names no author. */
export function authorLine(summary: Pick<TemplateSummary, "authors">): string | null {
  const names = (summary.authors ?? []).map((author) => author.name.trim()).filter(Boolean);
  return names.length ? `by ${names.join(", ")}` : null;
}

const BYLINE_NAMES = 3;

/** The worker names, then skill names, the card shows, and how many it leaves out. */
function rosterText(summary: Pick<TemplateSummary, "workers" | "worker_names" | "skills" | "skill_names">): string | null {
  const names = [...(summary.worker_names ?? []).map((name) => roleDisplayLabel(name)), ...(summary.skill_names ?? [])];
  const shown = names.slice(0, BYLINE_NAMES);
  if (shown.length === 0) return null;
  const more = summary.workers + (summary.skills ?? 0) - shown.length;
  return [...shown, ...(more > 0 ? [`${more} more`] : [])].join(", ");
}

/** The gallery card's meta line: "by Jane Doe · Researcher, Checker, source-check, 2 more", or "by you" for the owner's own; each part only when there is one. */
export function cardByline(summary: Pick<TemplateSummary, "authors" | "workers" | "worker_names" | "skills" | "skill_names">, mine = false): string | null {
  const first = (summary.authors ?? []).map((author) => author.name.trim()).find(Boolean);
  const by = mine ? "by you" : first ? `by ${first}` : null;
  const parts = [by, rosterText(summary)].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

export interface AuthorRow { name: string; role: string | null; url: string | null }

export function authorRows(summary: Pick<TemplateSummary, "authors">): AuthorRow[] {
  return (summary.authors ?? []).map((author: PackageAuthor) => ({
    name: author.name,
    role: author.role && author.role !== "author" ? author.role : null,
    url: safeUrl(author.url) ?? (author.email ? `mailto:${author.email}` : null),
  }));
}

const LINK_LABELS = [["homepage", "Homepage"], ["repository", "Repository"], ["issues", "Issues"], ["docs", "Docs"]] as const;

/** Only http(s) links leave the app; anything else in a shared package is shown as text, never followed. */
export function safeUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.href : null;
  } catch {
    return null;
  }
}

export function linkRows(summary: Pick<TemplateSummary, "links">): { label: string; url: string }[] {
  const links = summary.links;
  if (!links) return [];
  return LINK_LABELS.flatMap(([key, label]) => {
    const url = safeUrl(links[key]);
    return url ? [{ label, url }] : [];
  });
}

export function usedByLine(item: Pick<TemplatePayloadItem, "used_by">): string | null {
  const roles = item.used_by ?? [];
  return roles.length ? `used by ${roles.map((role) => roleDisplayLabel(role)).join(", ")}` : null;
}

function askText(ask: TemplateAsk): string {
  switch (ask.kind) {
    case "network": return ask.hosts?.length ? `network for ${ask.hosts.join(", ")}` : "network";
    case "repo_write": return "edits to the project";
    case "shell": return "shell commands";
    case "browser": return "a browser";
    case "secret": return ask.name;
    case "tool": return ask.tool;
    case "script": return ask.item;
  }
}

/** The muted one-line asks summary of a shared package card. */
export function shareAsksSummary(asks: readonly TemplateAsk[]): string | null {
  const secrets = asks.filter((ask) => ask.kind === "secret").length;
  const scripts = asks.filter((ask) => ask.kind === "script").length;
  const rest = asks.filter((ask) => ask.kind !== "secret" && ask.kind !== "script").map(askText);
  const parts = [...rest, ...(secrets ? [secrets === 1 ? "1 secret" : `${secrets} secrets`] : [])];
  const sentences = [
    parts.length ? `Asks for ${parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0]}.` : "",
    scripts ? `Runs ${scripts === 1 ? "a script" : `${scripts} scripts`} on start.` : "",
  ].filter(Boolean);
  return sentences.length ? sentences.join(" ") : null;
}

/** What an update would change, line by line; active Teams keep the version they started with. */
export function updateDiffLines(diff: TemplateUpdateDiff): string[] {
  const list = (label: string, items: readonly string[] | undefined) => items?.length ? [`${label}: ${items.join(", ")}`] : [];
  return [
    ...list("Roles added", diff.roles_added?.map(kindDisplayName)),
    ...list("Roles removed", diff.roles_removed?.map(kindDisplayName)),
    ...list("Roles changed", diff.roles_changed?.map(kindDisplayName)),
    ...list("Brings now", diff.payload_added?.map((item) => item.label)),
    ...list("No longer brings", diff.payload_removed?.map((item) => item.label)),
    ...list("New asks", diff.asks_added?.map(askText)),
  ];
}

export function isTemplateFileName(name: string): boolean {
  return /\.team(\.ya?ml)?$/i.test(name);
}

/** A required secret starts on the vault entry of the same name when there is one; an optional secret starts skipped. */
export function initialSecretBindings(asks: readonly TemplateAsk[], vaultNames: readonly string[]): Record<string, string> {
  return Object.fromEntries(asks.flatMap((ask) => ask.kind === "secret" && ask.required && vaultNames.includes(ask.name) ? [[ask.name, ask.name]] : []));
}

export interface HarnessRequirement { harness: string; installed: boolean }

/** `requires.harnesses` against this machine; a harness is missing when the preview names it as a local need. */
export function harnessRequirements(required: readonly string[], missing: ReadonlySet<string>): HarnessRequirement[] {
  return required.map((harness) => ({ harness, installed: !missing.has(harness) }));
}

export const SAVE_VERSION_DEFAULT = "1.0.0";

/** The editable "What it does" seed; the server refuses an empty description, even for a review. */
export function distillDescription(teamName: string): string {
  return `Starts a team like ${teamName.trim() || "this one"}.`;
}

/** A package name the owner can keep: their own namespace and a slug of the Team's name. */
export function distillName(teamName: string): string {
  const slug = teamName.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "team";
  return `me/${slug}`;
}
