export interface RouteContract { method: string; path: string }
export function rustRouteContracts(source: string): RouteContract[] {
  const contracts: RouteContract[] = [];
  const routes = /\.route\(\s*"([^"]+)"\s*,/g;
  let match: RegExpExecArray | null;
  while ((match = routes.exec(source))) {
    let depth = 1;
    let quoted = false;
    let end = routes.lastIndex;
    for (; end < source.length && depth; end++) {
      const char = source[end];
      if (char === '"' && source[end - 1] !== "\\") quoted = !quoted;
      if (!quoted) { if (char === "(") depth++; if (char === ")") depth--; }
    }
    const body = source.slice(routes.lastIndex, end - 1);
    for (const method of body.matchAll(/\b(get|post|put|patch|delete)\(/g)) contracts.push({ method: method[1].toUpperCase(), path: match[1] });
  }
  return contracts;
}
export function routeMatches(contract: RouteContract, method: string, path: string): boolean {
  if (contract.method !== method) return false;
  const actual = path.split("?")[0].split("/");
  const expected = contract.path.split("/");
  return actual.length === expected.length && expected.every((part, i) => /^\{[^}]+\}$/.test(part) ? actual[i].length > 0 : actual[i] === part);
}
