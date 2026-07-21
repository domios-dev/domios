import { AgentTreeRow } from "../../ds/AgentTreeRow";
import type { Specimen } from "../types";
export default { id:"agent-tree-row", title:"Agent tree row", category:"Sidebar", where:"src/components/Sidebar/TeamUnit.tsx", states:[{id:"working",label:"Working",props:{}},{id:"idle",label:"Idle",props:{status:"idle"}},{id:"selected",label:"Selected",props:{selected:true}},{id:"selected-idle",label:"Selected idle",props:{selected:true,status:"idle"}}], Component:AgentTreeRow } satisfies Specimen;
