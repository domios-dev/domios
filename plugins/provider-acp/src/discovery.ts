import {ioThen, type HostIoSteps, type AgentCommandCtx, type FetchResult, type Host} from "@codeterm/plugin-sdk";
import {registryBounds, saveRegistryCache, searchRegistryCache, type RegistryCachePort} from "./registry";
const indexUrl = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
export function registryCache(host: Pick<Host,"dataDir" | "io">): RegistryCachePort {
  const path = `${host.dataDir()}/acp/cache/registry-v1.json`;
  return {read: () => host.io.fs.readFileHead(path, registryBounds.bytes + 1), write: text => host.io.fs.writeFileAtomic(path,text)};
}
export function* discoveryCommand(host: Host, ctx: AgentCommandCtx): HostIoSteps<{result: string} | {error: string}> {
  const cache = registryCache(host);
  const result = function* () { return {result: JSON.stringify(yield* searchRegistryCache(cache,ctx.args.join(" ")))}; };
  try {
    if (ctx.verb === "registry-search") return yield* result();
    if (ctx.verb === "registry-refresh") {
      return yield* ioThen(host.io.fetch({url:indexUrl, timeoutMs:15000}), function* (response: FetchResult) {
        if (response.status !== 200 || !response.body || response.error) return {error:"ACP registry unavailable; the previous cache is retained"};
        try {yield* saveRegistryCache(cache,response.body); return yield* result();}
        catch {return {error:"Invalid ACP registry; the previous cache is retained"};}
      });
    }
    return {error:"Unknown ACP command"};
  } catch {return {error:"ACP registry cache is unavailable or invalid"};}
}
