export interface OutlineSymbol {
  kind: "function" | "class";
  name: string;
  params: string;
  returns: string | null;
}

const PYTHON_DEF = /^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(([^)]*)\)\s*(?:->\s*([^:]+))?:/;
const PYTHON_CLASS = /^\s*class\s+([A-Za-z_]\w*)/;
const SCRIPT_FUNCTION = /\bfunction\s*\*?\s*([A-Za-z_$][\w$]*)?\s*\(([^)]*)\)\s*(?::\s*([^{=]+?))?\s*\{/;
const SCRIPT_ARROW = /\b(?:const|let)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?\(([^)]*)\)\s*(?::\s*([^=]+?))?\s*=>/;
const SCRIPT_CLASS = /\bclass\s+([A-Za-z_$][\w$]*)/;

function clean(value: string | undefined): string {
  return (value ?? "").replace(/\s+/g, " ").trim();
}

/** Top-level functions and classes a reader looks for first, read from the source text without executing it. */
export function codeOutline(source: string, language: string | undefined): OutlineSymbol[] {
  const lang = (language ?? "").toLowerCase();
  const python = lang === "python" || lang === "py";
  const script = ["ts", "typescript", "js", "javascript", "tsx", "jsx", "mjs"].includes(lang);
  if (!python && !script) return [];
  const symbols: OutlineSymbol[] = [];
  for (const line of source.split("\n")) {
    const match = python ? PYTHON_DEF.exec(line) : SCRIPT_FUNCTION.exec(line) ?? SCRIPT_ARROW.exec(line);
    if (match) symbols.push({ kind: "function", name: match[1] ?? (line.includes("export default") ? "default" : "anonymous"), params: clean(match[2]), returns: clean(match[3]) || null });
    const type = (python ? PYTHON_CLASS : SCRIPT_CLASS).exec(line);
    if (type) symbols.push({ kind: "class", name: type[1], params: "", returns: null });
  }
  return symbols.slice(0, 24);
}
