import { createRequire } from "node:module";
import type { Plugin } from "vite";
import { ROOT_CLASS, STAGE_CONTAINER, STAGE_VH, STAGE_VW } from "../rootClass";

type Component = { type: string; kind?: string; name?: string; selectors?: Component[][]; selector?: Component[] };

// Vite's own lightningcss; the repo does not depend on it directly.
const lightningcss = createRequire(require.resolve("vite"))("lightningcss") as {
  transform(options: { filename: string; code: Uint8Array; minify: boolean; errorRecovery: boolean; visitor: { Selector: (s: Component[]) => Component[] } }): { code: Uint8Array };
};
const BASE_LAYER = "domios-film-base";

const isDocumentRoot = (c: Component) => (c.type === "pseudo-class" && c.kind === "root") || (c.type === "type" && (c.name === "html" || c.name === "body"));

/** `:root`, `html` and `body` all become the film root, so `html body x` collapses to one compound. */
function scopeSelector(selector: Component[]): Component[] {
  const out: Component[] = [];
  for (const c of selector) {
    const next = isDocumentRoot(c) ? { type: "class", name: ROOT_CLASS } : c;
    if (next.selectors && Array.isArray(next.selectors[0])) next.selectors = next.selectors.map(scopeSelector);
    if (next.selector) next.selector = scopeSelector(next.selector);
    const prev = out[out.length - 1];
    const prevRoot = out[out.length - 2];
    if (next.type === "class" && next.name === ROOT_CLASS && prev?.type === "combinator" && prevRoot?.type === "class" && prevRoot.name === ROOT_CLASS) {
      out.pop();
      continue;
    }
    if (next.type === "class" && next.name === ROOT_CLASS && out.some((x) => x.type === "class" && x.name === ROOT_CLASS) && prev?.type !== "combinator") continue;
    out.push(next);
  }
  return out;
}

interface Registered { name: string; inherits: boolean; initial?: string }

/** Registered properties are document-global and ignored in shadow trees; their initial values become a base layer. */
function extractProperties(css: string): { css: string; registered: Registered[] } {
  const registered: Registered[] = [];
  const rest = css.replace(/@property\s+(--[\w-]+)\s*\{([^}]*)\}/g, (_, name: string, body: string) => {
    const decl = (key: string) => body.match(new RegExp(`${key}\\s*:\\s*([^;]+)`))?.[1].trim();
    registered.push({ name, inherits: decl("inherits") === "true", initial: decl("initial-value") });
    return "";
  });
  return { css: rest, registered };
}

// A number in a declaration value: never inside an escaped selector such as `.p-\[0\.5rem\]` or `.h-\[min\(1px\,80vh\)\]`.
const valueNumber = (units: string) => new RegExp(String.raw`(?<=(?<!\\)[:\s(,*/+])(-?\d*\.?\d+)(${units})\b(?!\\)`, "g");
const REM = valueNumber("rem");
const VIEWPORT = valueNumber("[dsl]?(?:vw|vh|vmin|vmax)");
const PAGE_HEIGHT_FALLBACK = "var(--domios-film-max-height,100svh)";

/** The page's rem and viewport mean nothing inside a scaled 1920x1080 stage: rem is 16px, vw/vh are stage percent. */
export function stageUnits(css: string): string {
  const [head, ...rest] = css.split(PAGE_HEIGHT_FALLBACK);
  const convert = (part: string) =>
    part
      .replace(REM, (_, n: string) => `${+(Number(n) * 16).toFixed(4)}px`)
      .replace(VIEWPORT, (_, n: string, unit: string) => {
        const axis = unit.endsWith("vw") ? `var(${STAGE_VW})` : unit.endsWith("vh") ? `var(${STAGE_VH})` : `${unit.endsWith("vmin") ? "min" : "max"}(var(${STAGE_VW}),var(${STAGE_VH}))`;
        return `calc(${n}*${axis})`;
      });
  return [head, ...rest].map(convert).join(PAGE_HEIGHT_FALLBACK);
}

const WIDTH_QUERY = /^(not all and )?(\(width[<>]=?\d+px\))$/;

/** Width media queries become queries on the stage; hover/pointer answer as the desktop app the film shows. */
export function stageMedia(css: string): string {
  const pxPrelude = css.replace(/@(media|container) ([^{]+)\{/g, (_, kind: string, prelude: string) =>
    `@${kind} ${prelude.replace(/(\d*\.?\d+)rem\b/g, (__, n: string) => `${+(Number(n) * 16).toFixed(4)}px`)}{`);
  return pxPrelude.replace(/@media ([^{]+)\{/g, (rule, prelude: string) => {
    const containers: string[] = [];
    const media: string[] = [];
    for (const part of prelude.split(",").map((p) => p.trim())) {
      if (/^\((hover:hover|pointer:fine)\)$/.test(part)) return "@media all{";
      if (/^\((hover:none|pointer:coarse)\)$/.test(part)) continue;
      const width = part.match(WIDTH_QUERY);
      if (width) containers.push(width[1] ? `(not ${width[2]})` : width[2]);
      else media.push(part);
    }
    if (containers.length && media.length) return rule;
    if (containers.length) return `@container ${STAGE_CONTAINER} ${containers.join(" or ")}{`;
    return media.length ? `@media ${media.join(",")}{` : "@media not all{";
  });
}

/** Rewrites the bundled product CSS so it applies only inside the film's shadow root and inherits nothing from the page. */
export function scopeCss(css: string, usedVariables: Iterable<string>): string {
  const { css: unregistered, registered } = extractProperties(css);
  const { code } = lightningcss.transform({
    filename: "film.css",
    code: Buffer.from(unregistered),
    minify: true,
    errorRecovery: true,
    visitor: { Selector: scopeSelector },
  });
  const scoped = stageMedia(stageUnits(Buffer.from(code).toString()));
  const own = new Set(registered.map((p) => p.name));
  const blocked = [...new Set(usedVariables)].filter((name) => !own.has(name)).map((name) => `${name}:initial`);
  const perElement = registered.filter((p) => !p.inherits).map((p) => `${p.name}:${p.initial ?? "initial"}`);
  const inherited = registered.filter((p) => p.inherits).map((p) => `${p.name}:${p.initial ?? "initial"}`);
  const base = `@layer ${BASE_LAYER}{.${ROOT_CLASS}{all:initial;${[...blocked, ...inherited].join(";")}}*,:before,:after,::backdrop{${perElement.join(";")}}}`;
  return base + scoped;
}

/** Emits the library's single stylesheet, scoped for the shadow root, under a stable name the module loads itself. */
export function shadowCss(fileName: string): Plugin {
  return {
    name: "film-shadow-css",
    apply: "build",
    enforce: "post",
    generateBundle(_, bundle) {
      const sheets = Object.values(bundle).filter((f) => f.type === "asset" && f.fileName.endsWith(".css"));
      if (sheets.length !== 1) this.error(`expected one stylesheet, found ${sheets.map((s) => s.fileName).join(", ") || "none"}`);
      const sheet = sheets[0] as Extract<(typeof bundle)[string], { type: "asset" }>;
      const used = new Set<string>();
      for (const f of Object.values(bundle)) {
        const text = f.type === "chunk" ? f.code : typeof f.source === "string" ? f.source : "";
        for (const m of text.matchAll(/--(?:ct|tw)-[\w-]+/g)) used.add(m[0]);
      }
      delete bundle[sheet.fileName];
      this.emitFile({ type: "asset", fileName, source: scopeCss(String(sheet.source), used) });
    },
  };
}
