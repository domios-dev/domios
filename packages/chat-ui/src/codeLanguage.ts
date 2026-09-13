const EXT_MAP: Record<string, string> = {
  js: "javascript",
  jsx: "jsx",
  ts: "typescript",
  tsx: "tsx",
  json: "json",
  css: "css",
  scss: "scss",
  html: "html",
  md: "markdown",
  markdown: "markdown",
  mmd: "mermaid",
  mermaid: "mermaid",
  rs: "rust",
  py: "python",
  ps1: "powershell",
  sh: "bash",
  bash: "bash",
  yml: "yaml",
  yaml: "yaml",
  toml: "toml",
  sql: "sql",
  go: "go",
  java: "java",
  c: "c",
  h: "c",
  cpp: "cpp",
  hpp: "cpp",
  cs: "csharp",
};

export function languageFromPath(path: string | null): string {
  if (!path) return "text";
  const ext = path.split(/[./\\]/).pop()?.toLowerCase();
  return ext ? (EXT_MAP[ext] ?? ext) : "text";
}

/** Language for a known file extension, or "text" so an unknown file stays unhighlighted. */
export function knownLanguageFromPath(path: string | null): string {
  const ext = path?.split(/[./\\]/).pop()?.toLowerCase();
  return ext ? EXT_MAP[ext] ?? "text" : "text";
}

function inferFromSource(sample: string): string | null {
  const s = sample.slice(0, 6000);
  if (
    /\b(className|onPointerDown|onClick|useState|useEffect|useRef|useMemo|useCallback|React\.)/.test(s)
    || /from\s+['"]react['"]/.test(s)
  ) {
    return "tsx";
  }
  if (/\b(fn |let mut |impl |use crate::|#\[derive)/.test(s)) return "rust";
  if (/^\s*#!\/usr\/bin\//m.test(s) || /\b(def |elif |import )/.test(s)) return "python";
  if (/\b(func |package |:=)/.test(s)) return "go";
  if (/\b(interface |type [A-Z]\w*\s*=|export type )/.test(s)) return "typescript";
  if (/\b(import .+ from |export (const|function|class) )/.test(s)) return "javascript";
  if (/^\s*---\s*$/m.test(s) && /:\s*$/.test(s)) return "yaml";
  if (/^\s*{\s*"/m.test(s) || /^\s*\[\s*{/m.test(s)) return "json";
  if (/^diff --git /m.test(s) || /^@@ /m.test(s)) return "diff";
  return null;
}

export function inferCodeLanguage(path: string | null, source: string): string {
  const fromPath = languageFromPath(path);
  if (fromPath !== "text") return fromPath;
  return inferFromSource(source) ?? "text";
}

export function languageForEdits(
  path: string | null,
  edits: Array<{ old: string; new: string }>,
): string {
  const combined = edits.map((e) => `${e.old}\n${e.new}`).join("\n");
  return inferCodeLanguage(path, combined);
}
