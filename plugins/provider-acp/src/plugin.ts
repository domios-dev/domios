import { runtimePlan, runtimeResponse, runtimeReduce } from "./runtime";
export { runtimePlan, runtimeResponse, runtimeReduce };
import type {AgentCommandCtx} from "@codeterm/plugin-sdk";
import {discoveryCommand} from "./discovery";
export function onAgentCommand(ctx: AgentCommandCtx) {return discoveryCommand(host,ctx);}
export default { runtimePlan, runtimeResponse, runtimeReduce, onAgentCommand };
