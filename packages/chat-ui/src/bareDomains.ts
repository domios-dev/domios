import tlds from "tlds";

const recognizedTlds = new Set(tlds);
const fileExtensions = new Set("ts tsx js jsx mjs cjs rs md mdx json jsonc py pyc rb go java kt swift cs cpp c h hpp bash zsh ps1 bat cmd css scss sass less html htm xml yaml yml toml ini conf cfg log txt csv sql lock svg png jpg jpeg gif webp ico pdf zip gz tar wasm map vue svelte ex exs erl beam lua pl r ipynb git env bak tmp bin exe dll dylib o a lib".split(" "));

export interface DomainMatch { start: number; end: number; text: string; url: string }

function stripPunctuation(value: string): string {
  let result = value.replace(/[.,;:!?]+$/, "");
  while (/[)\]}]$/.test(result)) {
    const close = result[result.length - 1];
    const open = ({ ")": "(", "]": "[", "}": "{" } as Record<string, string>)[close];
    if (result.split(close).length <= result.split(open).length) break;
    result = result.slice(0, -1).replace(/[.,;:!?]+$/, "");
  }
  return result;
}

export function matchBareDomains(text: string): DomainMatch[] {
  const matches: DomainMatch[] = [];
  const tokens = /(?:mailto:[^\s<>"'`]+|(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,63}(?::\d+)?(?:[/?#][^\s<>"'`]*)?)/gi;
  for (const match of text.matchAll(tokens)) {
    const start = match.index!;
    const before = text[start - 1] ?? "";
    if (/[\w@./\\:-]/.test(before)) continue;
    const value = stripPunctuation(match[0]);
    const after = text[start + value.length] ?? "";
    if (/[\w-]/.test(after) || after === "." && /[\w-]/.test(text[start + value.length + 1] ?? "") || after === "(" && !value.includes("/")) continue;
    if (value.startsWith("mailto:")) {
      if (/^mailto:[^@\s]+@[^@\s]+\.[a-z]+$/i.test(value)) matches.push({ start, end: start + value.length, text: value, url: value });
      continue;
    }
    const host = value.split(/[/?#:]/)[0];
    const suffix = host.slice(host.lastIndexOf(".") + 1).toLowerCase();
    if (/^(?:file|main|script|run|build|install|test)\.sh$/i.test(host)) continue;
    if (!recognizedTlds.has(suffix) || fileExtensions.has(suffix) || /^v?\d+(?:\.\d+)+$/i.test(host)) continue;
    matches.push({ start, end: start + value.length, text: value, url: `https://${value}` });
  }
  return matches;
}
