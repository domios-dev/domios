import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export function repoFile(relative: string): string {
  return readFileSync(path.join(REPO_ROOT, relative), "utf8");
}

/** Resolves a CSS length the way a browser does: rem against the root font size the app sets from the terminal font. */
export function lengthToPx(value: string, rootFontPx: number): number {
  const parsed = /^(-?[\d.]+)(px|rem)$/.exec(value.trim());
  if (!parsed) throw new Error(`unsupported media length: ${value}`);
  return parsed[2] === "px" ? Number(parsed[1]) : Number(parsed[1]) * rootFontPx;
}

export function matchesMediaQuery(query: string, widthPx: number, rootFontPx: number): boolean {
  const text = query.trim();
  const negation = /^not\s+all\s+and\s+/.exec(text);
  if (negation) return !matchesMediaQuery(text.slice(negation[0].length), widthPx, rootFontPx);
  const max = /^\(\s*max-width:\s*([^)]+)\)$/.exec(text);
  if (max) return widthPx <= lengthToPx(max[1], rootFontPx);
  const min = /^\(\s*min-width:\s*([^)]+)\)$/.exec(text);
  if (min) return widthPx >= lengthToPx(min[1], rootFontPx);
  const modern = /^\(\s*width\s*>=\s*([^)]+)\)$/.exec(text);
  if (modern) return widthPx >= lengthToPx(modern[1], rootFontPx);
  throw new Error(`unsupported media query: ${query}`);
}

export function customVariantQuery(css: string, name: string): string {
  const found = new RegExp(`@custom-variant\s+${name}\s+\(@media\s+(.+?)\);`).exec(css);
  if (!found) throw new Error(`globals.css declares no @custom-variant ${name}`);
  return found[1].trim();
}

/** Every hand-written block whose whole prelude is a single width condition — the shell's own responsive CSS. */
export function widthMediaPreludes(css: string): string[] {
  return [...css.matchAll(/^@media\s+(\((?:max|min)-width:[^)]+\))\s*\{/gm)].map((m) => m[1]);
}

export function classAttributeContaining(source: string, marker: string): string {
  const line = source.split(/\r?\n/).find((candidate) => candidate.includes(marker));
  if (!line) throw new Error(`no class attribute mentions ${marker}`);
  return line;
}

/** Tailwind's stock `sm`, kept here to prove the shell boundary no longer rides on a rem breakpoint. */
export const TAILWIND_DEFAULT_SM_QUERY = "(width >= 40rem)";

export const DEFAULT_TERMINAL_FONT_PX = 14;
export const ROOT_FONT_OFFSET_PX = 2;

export function rootFontPxFor(terminalFontPx: number): number {
  return terminalFontPx + ROOT_FONT_OFFSET_PX;
}
