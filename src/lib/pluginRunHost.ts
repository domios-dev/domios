import type { PluginDocument } from "./pluginDocument";
import type { OperationDecisionHost } from "./pluginOperationDecision";

export function pluginRunHost(pluginId: string, hostRequest: PluginDocument["hostRequest"]) {
  async function request(route: "/api/plugins/run-view" | "/api/plugins/operation-decision", body: unknown): Promise<unknown> {
    const {response,current} = await hostRequest(route, {headers:{"Content-Type":"application/json"},body:JSON.stringify(body)});
    const result: unknown = await response.json();
    return current() ? result : {ok:false,code:"denied",reason:"stale"};
  }
  const decide: OperationDecisionHost["send"] = decision => request("/api/plugins/operation-decision", {pluginId,...decision});
  return {resolve: (reference: string) => request("/api/plugins/run-view", {pluginId,reference}), decide};
}
