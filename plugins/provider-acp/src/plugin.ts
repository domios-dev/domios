import { runtimePlan, runtimeResponse, runtimeReduce } from "./runtime";
export { runtimePlan, runtimeResponse, runtimeReduce };
import type {AgentCommandCtx, FetchOpts, FetchResult} from "@codeterm/plugin-sdk";
import {discoveryCommand} from "./discovery";
import {readProfileLimits as readLimits, type ProfileLimitsRequest} from "./limits";
export function onAgentCommand(ctx: AgentCommandCtx) {return discoveryCommand(host,ctx);}
export function readProfileLimits(request: ProfileLimitsRequest, nowMs: number) {
  return readLimits(<T>(opts: FetchOpts, then: (result: FetchResult) => T) => host.fetch.async(opts, then), request, nowMs);
}
export default { runtimePlan, runtimeResponse, runtimeReduce, onAgentCommand, readProfileLimits };
