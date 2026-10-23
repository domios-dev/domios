const ATTACHMENT_TAIL = /^(?:\s*<domios\b(?=[^<>]*\stype="attachment")[^<>]*\/?>)+\s*$/i;

export function userEchoBody(content: string): string {
  const text = content.replace(/\r\n/g, "\n").trim();
  const wrapped = /^<pasted_content(?:\s+[^<>]*)?>([\s\S]*)<\/pasted_content(?:\s+[^<>]*)?>\s*(?:pasted by Domios)?$/.exec(text);
  if (wrapped) return wrapped[1].trim();
  // A provider records attachment markers after the wrapper of its bracketed paste.
  const beside = /^<pasted_content(?:\s+id="([^"<>]*)")?\s*>([\s\S]*?)<\/pasted_content(?:\s+id="([^"<>]*)")?\s*>([\s\S]*)$/.exec(text);
  if (beside && (beside[3] === undefined || beside[3] === beside[1]) && ATTACHMENT_TAIL.test(beside[4])) {
    return `${beside[2].trim()}\n${beside[4].trim()}`;
  }
  return text.replace(/\n+pasted by Domios$/, "").trim();
}
