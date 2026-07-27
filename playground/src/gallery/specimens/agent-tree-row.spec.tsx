import { AgentTreeRow } from "../../ds/AgentTreeRow";
import { defineSpecimen } from "../types";
const base = { name: "Motion Design Lead", role: "ORCHESTRATOR", model: "FABLE" };

export default defineSpecimen({ id:"agent-tree-row", title:"Agent tree row", category:"Sidebar", where:"src/components/Sidebar/TeamUnit.tsx", states:[{id:"working",label:"Working",props:{...base,status:"working"}},{id:"idle",label:"Idle",props:{...base,status:"idle"}},{id:"selected",label:"Selected",props:{...base,status:"working",selected:true}},{id:"selected-idle",label:"Selected idle",props:{...base,status:"idle",selected:true}}], Component:AgentTreeRow });
