import type { ComponentType } from "react";
import { AgentTreeRow } from "../../ds/AgentTreeRow";
type SpecimenDef = { id: string; title: string; where: string; states: Array<{ id: string; label: string; props: object }>; Component: ComponentType<any>; notes?: string };
export default { id:"agent-tree-row", title:"Agent tree row", where:"src/components/Sidebar/TeamUnit.tsx", states:[{id:"working",label:"Working",props:{}},{id:"idle",label:"Idle",props:{status:"idle"}},{id:"selected",label:"Selected",props:{selected:true}},{id:"selected-idle",label:"Selected idle",props:{selected:true,status:"idle"}}], Component:AgentTreeRow } satisfies SpecimenDef;
