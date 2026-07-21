import { AgentStatusRow } from "../../ds/AgentStatusRow";
import type { Specimen } from "../types";
const base = { providerName: "Claude Code", model: "fable", detail: "9m 23s · ↓ 35.3k tokens" };

export default { id:"agent-status-row", title:"Agent status row", category:"Status", where:"src/components/Terminal/AgentStateIndicator.tsx", states:[{id:"working",label:"Working",props:{...base,phase:"work",activity:"Planning gallery catalog…"}},{id:"thinking",label:"Thinking",props:{...base,phase:"thinking",activity:"Considering the next move…"}},{id:"streaming",label:"Streaming",props:{...base,phase:"streaming",activity:"Writing response…"}},{id:"idle",label:"Idle",props:{...base,phase:"idle",activity:"Last turn",detail:"Churned for 1m 37s"}}], Component:AgentStatusRow, notes:"Liquid dots retain the product work, thinking, streaming, and idle beats." } satisfies Specimen;
