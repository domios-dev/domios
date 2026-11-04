import type {AgentCommandCtx, FetchResult, Host} from "@codeterm/plugin-sdk";
import {registryBounds, saveRegistryCache, searchRegistryCache, type RegistryCachePort} from "./registry";
const indexUrl = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
export function registryCache(host: Pick<Host,"dataDir" | "readFileHead" | "writeFileAtomic">): RegistryCachePort {
  const path = `${host.dataDir()}/acp/cache/registry-v1.json`;
  return {read: () => host.readFileHead(path, registryBounds.bytes + 1), write: text => host.writeFileAtomic(path,text)};
}
export function discoveryCommand(host: Host, ctx: AgentCommandCtx): {result: string} | {error: string} {
  const cache = registryCache(host);
  const result = () => ({result: JSON.stringify(searchRegistryCache(cache,ctx.args.join(" ")))});
  try {
    if (ctx.verb === "registry-search") return result();
    if (ctx.verb === "registry-refresh") {
      return host.fetch.async({url:indexUrl, timeoutMs:15000}, (response: FetchResult) => {
        if (response.status !== 200 || !response.body || response.error) return {error:"ACP registry unavailable; the previous cache is retained"};
        try {saveRegistryCache(cache,response.body); return result();}
        catch {return {error:"Invalid ACP registry; the previous cache is retained"};}
      });
    }
    return {error:"Unknown ACP command"};
  } catch {return {error:"ACP registry cache is unavailable or invalid"};}
}
