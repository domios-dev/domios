import { runtimePlan, runtimeResponse, runtimeReduce } from "./runtime";
export { runtimePlan, runtimeResponse, runtimeReduce };
import type {AgentCommandCtx} from "@codeterm/plugin-sdk";
import {discoveryCommand} from "./discovery";
import {readProfileLimits as readLimits, type LimitsPorts, type ProfileLimitsRequest} from "./limits";
export function onAgentCommand(ctx: AgentCommandCtx) {return discoveryCommand(host,ctx);}
const limitsPorts: LimitsPorts = {
  fetch: (opts, then) => host.fetch.async(opts, then),
  credentialPublic: (id, then) => host.io({op:"credentialPublic", id}, fields => then(fields as Readonly<Record<string,string>> | null)),
};
export function readProfileLimits(request: ProfileLimitsRequest, nowMs: number) {
  return readLimits(limitsPorts, request, nowMs);
}
export default { runtimePlan, runtimeResponse, runtimeReduce, onAgentCommand, readProfileLimits };
