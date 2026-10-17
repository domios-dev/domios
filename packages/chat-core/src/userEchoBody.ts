export function userEchoBody(content: string): string {
  const text = content.replace(/\r\n/g, "\n").trim();
  const wrapped = /^<pasted_content(?:\s+[^<>]*)?>([\s\S]*)<\/pasted_content(?:\s+[^<>]*)?>\s*(?:pasted by Domios)?$/.exec(text);
  return wrapped ? wrapped[1].trim() : text.replace(/\n+pasted by Domios$/, "").trim();
}
