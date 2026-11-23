import { readFileSync } from "node:fs";
import { basename } from "node:path";
import type { Plugin } from "vite";

const IMAGE = /\.(png|svg|jpe?g|webp|gif|avif)$/;
const URL_PATTERN = /new URL\((["'])(\.{1,2}\/[^"']+\.(?:png|svg|jpe?g|webp|gif|avif))\1,\s*import\.meta\.url\)/g;

/** Library mode inlines every asset as a data: URL; a site whose img-src is 'self' needs real files instead. */
export function fileAssets(): Plugin {
  return {
    name: "film-file-assets",
    apply: "build",
    enforce: "pre",
    transform(code, id) {
      if (id.includes("/node_modules/") || !code.includes("import.meta.url")) return null;
      const imports: string[] = [];
      const out = code.replace(URL_PATTERN, (_, __, path: string) => {
        const name = `__filmAsset${imports.length}`;
        imports.push(`import ${name} from ${JSON.stringify(path)};`);
        return `new URL(${name}, import.meta.url)`;
      });
      return imports.length ? { code: `${imports.join("\n")}\n${out}`, map: null } : null;
    },
    load(id) {
      if (id.includes("?") || !IMAGE.test(id)) return null;
      const ref = this.emitFile({ type: "asset", name: basename(id), source: readFileSync(id) });
      return `export default import.meta.ROLLUP_FILE_URL_${ref};`;
    },
  };
}
