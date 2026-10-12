const BLOCK_LINE = /^[\t ]*@(?:ds|ct)-([a-z][a-z0-9-]*)[\t ]+(\S(?:.*\S)?)[\t ]*$/;

/** Gives every standalone `@ds-` (or legacy `@ct-`) line its own paragraph so a lead sentence on the line above cannot swallow it. */
export function isolateBlockDirectives(text: string, inserted?: (offset: number) => void): string {
  if (!text.includes("@ds-") && !text.includes("@ct-")) return text;
  const out: string[] = [];
  let fence: string | null = null;
  let offset = 0;
  for (const line of text.split("\n")) {
    const marker = /^[\t ]*(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker && fence === null) fence = marker;
    else if (marker && fence !== null && marker[0] === fence[0] && marker.length >= fence.length) fence = null;
    const directive = fence === null && BLOCK_LINE.test(line.replace(/\r$/, ""));
    if (directive && out.length && out[out.length - 1].trim()) { out.push(""); inserted?.(offset); }
    out.push(line);
    if (directive) { out.push(""); inserted?.(offset + line.length); }
    offset += line.length + 1;
  }
  return out.join("\n");
}

